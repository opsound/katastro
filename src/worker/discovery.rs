use super::*;
use crate::{
    EngineProfile,
    training::{GRADE_VISITS, Phase, Trainer},
};

#[derive(Default)]
pub(super) struct Discovery {
    storage: (PathBuf, PathBuf),
    libraries: Vec<(String, Vec<PathBuf>)>,
    pending: std::collections::VecDeque<usize>,
    pub trainer: Option<Trainer>,
    profile: Option<EngineProfile>,
    completed: BTreeMap<NodeId, u64>,
    indexed_visits: u64,
}
impl Discovery {
    pub fn new(reviews: PathBuf, cache: PathBuf) -> Self {
        Self {
            storage: (reviews, cache),
            ..Default::default()
        }
    }
    pub fn libraries(&mut self, mut libraries: Vec<(String, Vec<PathBuf>)>) {
        // Review stores canonical source paths; normalize on this worker once,
        // rather than doing filesystem lookups for every analysis response.
        for (_, games) in &mut libraries {
            for path in games {
                if let Ok(canonical) = path.canonicalize() {
                    *path = canonical;
                }
            }
        }
        if self.libraries != libraries {
            self.indexed_visits = 0;
        }
        self.libraries = libraries;
        self.resume();
    }
    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn pause(&mut self) {
        self.trainer = None;
        self.pending.clear();
    }
    pub fn opened(&mut self) {
        self.pause();
        self.profile = None;
        self.completed.clear();
        self.indexed_visits = 0;
    }
    pub fn started(&mut self, profile: EngineProfile, values: &BTreeMap<NodeId, Analysis>) {
        self.profile = Some(profile);
        self.completed = values.iter().map(|(id, a)| (*id, a.visits)).collect();
    }
    pub fn completed(&mut self, node: NodeId, visits: u64) {
        self.completed
            .entry(node)
            .and_modify(|old| *old = (*old).max(visits))
            .or_insert(visits);
    }
    pub fn indexed(&self) -> bool {
        self.indexed_visits > 0
    }
    fn trainer(&self, profile: EngineProfile) -> Result<Trainer> {
        Trainer::new(
            &self.storage.0,
            &self.storage.1,
            &self
                .storage
                .0
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("Training"),
            profile,
        )
    }
    pub fn index_review(&mut self, source: &std::path::Path, doc: &Document) -> Result<()> {
        let visits = doc
            .mainline
            .iter()
            .map(|id| self.completed.get(id).copied().unwrap_or(0))
            .min()
            .unwrap_or(0);
        // An exact archive membership outranks a name shared with an opponent
        // on another server. Name matching also supports manually opened SGFs.
        let owner = self
            .libraries
            .iter()
            .find(|(account, games)| !account.is_empty() && games.iter().any(|p| p == source));
        let Some((account, _)) = owner.or_else(|| {
            self.libraries.iter().find(|(account, _)| {
                !account.is_empty()
                    && ["PB", "PW"].iter().any(|key| {
                        doc.nodes[0]
                            .property(key)
                            .is_some_and(|name| name.eq_ignore_ascii_case(account))
                    })
            })
        }) else {
            return Ok(());
        };
        if visits < GRADE_VISITS.max(self.indexed_visits.saturating_mul(4)) {
            return Ok(());
        }
        let Some(profile) = &self.profile else {
            return Ok(());
        };
        let mut trainer = self.trainer(profile.clone())?;
        trainer.prepare(vec![source.into()], account.clone(), true)?;
        // Only cached, durable results are used here; foreground review owns the engine.
        if trainer.snapshot().phase == Phase::Idle {
            self.indexed_visits = visits;
        }
        Ok(())
    }
    pub fn start_idle(&mut self, config: &EngineConfig) -> Result<()> {
        if self.pending.is_empty()
            || self
                .trainer
                .as_ref()
                .is_some_and(|t| t.snapshot().phase != Phase::Idle)
        {
            return Ok(());
        }
        let (account, games) = &self.libraries[self.pending.pop_front().unwrap()];
        if account.is_empty()
            || games.is_empty()
            || !config.model.is_file()
            || !config.executable.is_file()
        {
            return Ok(());
        }
        let mut trainer = self
            .trainer(Engine::profile(config)?)?
            .with_analysis_settings(&config.analysis)?;
        trainer.prepare(games.clone(), account.clone(), false)?;
        self.trainer = Some(trainer);
        Ok(())
    }
    pub fn reconfigure(&mut self) {
        self.opened();
        self.pending = (0..self.libraries.len()).collect();
    }
    pub fn resume(&mut self) {
        self.pending = (0..self.libraries.len()).collect();
    }
}
