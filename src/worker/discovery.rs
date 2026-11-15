use super::*;
use crate::{
    EngineProfile,
    training::{GRADE_VISITS, Phase, Trainer},
};

#[derive(Default)]
pub(super) struct Discovery {
    storage: (PathBuf, PathBuf),
    games: Vec<PathBuf>,
    account: String,
    pending: bool,
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
    pub fn library(&mut self, games: Vec<PathBuf>, account: String) {
        if self.account != account {
            self.indexed_visits = 0;
        }
        self.games = games;
        self.account = account;
        self.pending = true;
    }
    pub fn pause(&mut self) {
        self.trainer = None;
        self.pending = false;
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
        if self.account.is_empty()
            || visits < GRADE_VISITS.max(self.indexed_visits.saturating_mul(4))
        {
            return Ok(());
        }
        let Some(profile) = &self.profile else {
            return Ok(());
        };
        let mut trainer = self.trainer(profile.clone())?;
        trainer.prepare(vec![source.into()], self.account.clone(), true)?;
        // Only cached, durable results are used here; foreground review owns the engine.
        if trainer.snapshot().phase == Phase::Idle {
            self.indexed_visits = visits;
        }
        Ok(())
    }
    pub fn start_idle(&mut self, config: &EngineConfig) -> Result<()> {
        if !self.pending
            || self
                .trainer
                .as_ref()
                .is_some_and(|t| t.snapshot().phase != Phase::Idle)
        {
            return Ok(());
        }
        self.pending = false;
        if self.account.is_empty()
            || self.games.is_empty()
            || !config.model.is_file()
            || !config.executable.is_file()
        {
            return Ok(());
        }
        let mut trainer = self
            .trainer(Engine::profile(config)?)?
            .with_analysis_settings(&config.analysis)?;
        trainer.prepare(self.games.clone(), self.account.clone(), false)?;
        self.trainer = Some(trainer);
        Ok(())
    }
    pub fn reconfigure(&mut self) {
        self.opened();
        self.pending = true;
    }
    pub fn resume(&mut self) {
        self.pending = true;
    }
}
