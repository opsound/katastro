//! Library exercises. All evaluations use Black's perspective internally.
use crate::{Analysis, Color, Document, EngineProfile, Point, Result, Review, digest};
use rand::{SeedableRng, rngs::StdRng};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
mod catalog;
pub mod client;
mod grading;
mod queue;
mod reveal;
pub use grading::{Comparison, Evaluation, MoveLoss, Report};
#[cfg(feature = "desktop")]
pub mod view;
const SCREEN_VISITS: u64 = crate::analysis::MIN_CACHE_VISITS;
const POLICY: &str = "training-score-v2";
const MAJOR_LOSS: f64 = 5.0;
pub const GRADE_VISITS: u64 = 512;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Screening,
    Searching,
    YourTurn,
    Opponent,
    Grading,
    Failed,
    Passed,
    Revealing,
    Revealed,
}
#[derive(Clone, Debug)]
pub struct Settings {
    pub moves: usize,
    pub strict: bool,
    pub include_completed: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            moves: 5,
            strict: true,
            include_completed: false,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub phase: Phase,
    pub document: Option<Document>,
    pub board: Option<crate::Board>,
    pub status: String,
    pub error: Option<String>,
    pub played: usize,
    /// Stones played by either side during this attempt, excluding the exercise prefix.
    pub stones_placed: usize,
    pub target: usize,
    pub report: Option<Report>,
    pub reveal: Option<(PathBuf, usize)>,
    pub lines: Vec<Vec<Option<Point>>>,
}
impl Snapshot {
    pub(crate) fn searching(status: impl Into<String>) -> Self {
        Self {
            phase: Phase::Searching,
            status: status.into(),
            ..Default::default()
        }
    }
}

struct Problem {
    doc: Document,
    index: usize,
    color: Color,
    key: String,
    group: String,
    prefix: Document,
}
enum Kind {
    Evaluation {
        key: String,
        position: String,
        visits: u64,
    },
    Opponent,
    Lines,
}
struct Job {
    query: Value,
    kind: Kind,
}
pub struct Trainer {
    state: Snapshot,
    review: Review,
    output: PathBuf,
    profile: EngineProfile,
    settings: Settings,
    account: String,
    games: VecDeque<PathBuf>,
    game: Option<catalog::Game>,
    confirming: Option<usize>,
    problem: Option<Problem>,
    memo: HashMap<String, Analysis>,
    queue: VecDeque<Job>,
    pending: HashMap<String, Job>,
    sequence: u64,
    instance: u64,
    scanned: usize,
    skipped: usize,
    library: Vec<PathBuf>,
    seen: HashSet<String>,
    preparing: bool,
    refresh: bool,
    report_id: Option<i64>,
    rng: StdRng,
    parallel_positions: usize,
}
impl Trainer {
    pub(crate) fn profile(&self) -> &EngineProfile {
        &self.profile
    }

    /// Share engine capacity; exercise grading retains its fixed visit budget.
    pub fn with_analysis_settings(
        mut self,
        settings: &crate::engine::AnalysisSettings,
    ) -> Result<Self> {
        settings.validate()?;
        self.parallel_positions = settings.parallel_positions;
        Ok(self)
    }
    /// Make problem order reproducible for a session.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.rng = StdRng::seed_from_u64(seed);
        self
    }
    pub fn new(
        reviews: &Path,
        cache: &Path,
        output: &Path,
        profile: EngineProfile,
    ) -> Result<Self> {
        static INSTANCE: AtomicU64 = AtomicU64::new(0);
        let mut review = Review::new(reviews, cache)?;
        let migration = review
            .reviews
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        migration.execute_batch("CREATE TABLE IF NOT EXISTS training_attempts (id INTEGER PRIMARY KEY, problem TEXT NOT NULL, outcome TEXT NOT NULL, report TEXT, document TEXT NOT NULL);")?;
        let grouped: bool = migration.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('training_attempts') WHERE name='group_key')", [], |row| row.get(0),
        )?;
        if !grouped {
            migration.execute(
                "ALTER TABLE training_attempts ADD COLUMN group_key TEXT",
                [],
            )?;
        }
        migration.commit()?;
        Ok(Self {
            state: Snapshot::default(),
            review,
            output: output.into(),
            profile,
            settings: Settings::default(),
            account: String::new(),
            games: VecDeque::new(),
            game: None,
            confirming: None,
            problem: None,
            memo: HashMap::new(),
            queue: VecDeque::new(),
            pending: HashMap::new(),
            sequence: 0,
            instance: INSTANCE.fetch_add(1, Ordering::Relaxed),
            scanned: 0,
            skipped: 0,
            library: Vec::new(),
            seen: HashSet::new(),
            preparing: false,
            refresh: false,
            report_id: None,
            rng: StdRng::from_rng(&mut rand::rng()),
            parallel_positions: crate::engine::DEFAULT_PARALLEL_POSITIONS,
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut state = self.state.clone();
        state.board = state
            .document
            .as_ref()
            .and_then(|d| d.board(d.selected).ok());
        state
    }
    pub fn begin(
        &mut self,
        games: Vec<PathBuf>,
        account: String,
        settings: Settings,
    ) -> Result<()> {
        self.seen.clear();
        self.initialize(games, account, settings)?;
        self.find_prepared()
    }
    pub fn next_problem(&mut self) -> Result<()> {
        if !matches!(
            self.state.phase,
            Phase::Passed | Phase::Failed | Phase::Revealed
        ) {
            return Err("Finish this attempt before continuing".into());
        }
        self.seen
            .insert(self.problem.as_ref().unwrap().group.clone());
        self.initialize(
            self.library.clone(),
            self.account.clone(),
            self.settings.clone(),
        )?;
        self.find_prepared()
    }
    fn initialize(
        &mut self,
        games: Vec<PathBuf>,
        account: String,
        settings: Settings,
    ) -> Result<()> {
        if account.trim().is_empty() {
            return Err("Enter the player name used in your game records".into());
        }
        if !(1..=20).contains(&settings.moves) {
            return Err("Choose between 1 and 20 moves per attempt".into());
        }
        self.cancel();
        self.account = account.trim().into();
        self.settings = settings;
        self.library = games.clone();
        self.games = games.into();
        self.preparing = false;
        self.refresh = false;
        self.scanned = 0;
        self.skipped = 0;
        self.state.target = self.settings.moves;
        self.state.phase = Phase::Searching;
        Ok(())
    }
    fn start_problem(&mut self, doc: Document, candidate: catalog::Candidate) -> Result<()> {
        let index = candidate.index;
        self.problem = Some(Problem {
            prefix: prefix(&doc, doc.mainline[index])?,
            color: player_color(&doc, &self.account).unwrap(),
            key: self.problem_key(&doc, index)?,
            group: candidate.group,
            index,
            doc: doc.clone(),
        });
        self.reset_attempt();
        self.prefetch_recorded(&doc, index, self.settings.moves * 2)
    }
    pub fn accept(&mut self, reply: &Value, sample: f64) -> Result<()> {
        let Some(id) = reply["id"].as_str() else {
            return Err("Engine response has no ID".into());
        };
        let Some(job) = self.pending.get(id) else {
            return Ok(());
        };
        if let Some(error) = reply.get("error") {
            return Err(format!("KataGo: {error}").into());
        }
        if reply["isDuringSearch"] != false || reply["turnNumber"] != job.query["analyzeTurns"][0] {
            return Ok(());
        }
        let job = self.pending.remove(id).unwrap();
        let had_report = self.state.report.is_some();
        match job.kind {
            Kind::Evaluation {
                key,
                position,
                visits,
            } => {
                let a = Analysis::from_reply(
                    reply,
                    job.query["boardXSize"].as_u64().unwrap() as usize,
                )?;
                if a.visits < visits {
                    return Err("KataGo returned an incomplete training evaluation".into());
                }
                self.review.store_analysis(&position, &a)?;
                self.review.store_analysis(&key, &a)?;
                self.memo.insert(key, a);
            }
            Kind::Opponent => {
                let doc = self.state.document.as_mut().ok_or("No exercise")?;
                let point = sample_policy(doc, &reply["humanPolicy"], sample)?;
                doc.append_move(point)?;
                self.state.stones_placed += usize::from(point.is_some());
                let doc = doc.clone();
                if self.state.played < self.settings.moves && !two_passes(&doc) {
                    self.need_attempt(&doc)?;
                    self.state.phase = Phase::YourTurn;
                    self.state.status = "Your move · evaluations stay hidden".into();
                } else {
                    self.need(&doc, doc.selected, GRADE_VISITS, false)?;
                    self.state.phase = Phase::Grading;
                    self.state.status = "Comparing losses on your moves…".into();
                }
            }
            Kind::Lines => self.finish_reveal(reply)?,
        }
        self.advance()?;
        if had_report && matches!(self.state.phase, Phase::Passed | Phase::Failed) {
            self.update_comparison()?;
        }
        Ok(())
    }
    fn advance(&mut self) -> Result<()> {
        if self.state.phase == Phase::Screening && self.queue.is_empty() && self.pending.is_empty()
        {
            self.state.phase = Phase::Idle;
            self.state.status = "Library screening complete".into();
        }
        while self.state.phase == Phase::Searching
            && self.queue.is_empty()
            && self.pending.is_empty()
        {
            self.search_next()?;
        }
        if self.state.phase == Phase::Grading {
            let end = self.original_end()?;
            if self.ready_to_grade(end)? {
                self.grade(end)?;
            }
        }
        Ok(())
    }
    fn reset_attempt(&mut self) {
        self.report_id = None;
        self.state = Snapshot {
            phase: Phase::YourTurn,
            document: Some(self.problem.as_ref().unwrap().prefix.clone()),
            target: self.settings.moves,
            status: "Your move · improve on your recorded moves".into(),
            ..Default::default()
        };
    }
    pub fn play(&mut self, point: Option<Point>) -> Result<()> {
        if self.state.phase != Phase::YourTurn {
            return Err("Wait for your turn".into());
        }
        let doc = self.state.document.as_mut().unwrap();
        doc.append_move(point)?;
        self.state.stones_placed += usize::from(point.is_some());
        let doc = doc.clone();
        self.state.played += 1;
        self.state.phase = Phase::Opponent;
        self.state.status = "1 dan is thinking…".into();
        self.need_attempt(&doc)?;
        if two_passes(&doc) {
            self.state.phase = Phase::Grading;
        } else {
            let mut query = self.query(&doc, doc.selected, 1)?;
            query["includePolicy"] = json!(true);
            query["overrideSettings"] = json!({"humanSLProfile":"rank_1d"});
            self.queue.push_front(Job {
                query,
                kind: Kind::Opponent,
            });
        }
        self.advance()
    }
    pub fn retry(&mut self) -> Result<()> {
        if self.state.phase != Phase::Failed {
            return Err("Retry is available after a failed attempt".into());
        }
        self.queue.clear();
        self.pending.clear();
        self.reset_attempt();
        let p = self.problem.as_ref().unwrap();
        self.prefetch_recorded(&p.doc.clone(), p.index, self.settings.moves * 2)
    }
    pub fn cancel(&mut self) {
        self.report_id = None;
        self.queue.clear();
        self.pending.clear();
        self.game = None;
        self.problem = None;
        self.confirming = None;
        self.games.clear();
        self.memo.clear();
        self.state = Snapshot::default();
    }
}
fn score(a: &Analysis, color: Color) -> f64 {
    color.perspective(a.score_lead)
}
fn player_color(doc: &Document, account: &str) -> Option<Color> {
    [Color::Black, Color::White].into_iter().find(|c| {
        doc.nodes[0]
            .property(if *c == Color::Black { "PB" } else { "PW" })
            .is_some_and(|s| s.trim().eq_ignore_ascii_case(account))
    })
}
fn horizon(doc: &Document, index: usize, plies: usize) -> Option<usize> {
    if plies == 0 {
        return Some(doc.mainline[index]);
    }
    doc.mainline
        .iter()
        .skip(index + 1)
        .copied()
        .filter(|id| doc.nodes[*id].played.is_some())
        .nth(plies - 1)
}
fn prefix(doc: &Document, node: usize) -> Result<Document> {
    let path = doc.path(node)?;
    let mut prefix = doc.clone();
    prefix.nodes = path
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let mut n = doc.nodes[*id].clone();
            n.parent = i.checked_sub(1);
            n.children = if i + 1 < path.len() {
                vec![i + 1]
            } else {
                vec![]
            };
            n.props.retain(|k, _| {
                [
                    "GM", "FF", "SZ", "KM", "RU", "HA", "AB", "AW", "AE", "PL", "B", "W",
                ]
                .contains(&k.as_str())
            });
            n
        })
        .collect();
    prefix.mainline = (0..path.len()).collect();
    prefix.selected = path.len() - 1;
    Ok(prefix)
}
pub(crate) fn two_passes(doc: &Document) -> bool {
    let n = &doc.nodes[doc.selected];
    n.played.is_some_and(|m| m.point.is_none())
        && n.parent
            .is_some_and(|id| doc.nodes[id].played.is_some_and(|m| m.point.is_none()))
}
pub(crate) fn sample_policy(doc: &Document, policy: &Value, sample: f64) -> Result<Option<Point>> {
    if !(0.0..1.0).contains(&sample) {
        return Err("Invalid random sample".into());
    }
    let policy = policy
        .as_array()
        .filter(|p| p.len() == doc.size * doc.size + 1)
        .ok_or("HumanSL policy is missing; use the HumanSL model with KataGo 1.15 or newer")?;
    let mut legal = Vec::new();
    let mut sum = 0.0;
    for (i, weight) in policy.iter().enumerate() {
        let w = weight
            .as_f64()
            .filter(|w| w.is_finite() && (-1.0..=1.0).contains(w))
            .ok_or("Invalid HumanSL probability")?;
        if w <= 0.0 {
            continue;
        }
        let point = if i == doc.size * doc.size {
            None
        } else {
            Some(Point::new(i % doc.size, i / doc.size))
        };
        if doc.clone().append_move(point).is_ok() {
            sum += w;
            legal.push((point, w));
        }
    }
    if sum <= 0.0 {
        return Err("HumanSL returned no legal moves".into());
    }
    let mut target = sample * sum;
    for (point, w) in &legal {
        target -= w;
        if target < 0.0 {
            return Ok(*point);
        }
    }
    Ok(legal.last().unwrap().0)
}
