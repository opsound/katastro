//! Library exercises. All evaluations use Black's perspective internally.
use crate::{Analysis, Color, Document, EngineProfile, Point, Result, Review, digest};
use rusqlite::{Connection, OptionalExtension, params};
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
#[cfg(feature = "desktop")]
pub mod view;
const SCREEN_VISITS: u64 = 64;
pub const GRADE_VISITS: u64 = 512;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
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
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct MoveLoss {
    pub winrate: f64,
    pub score: f64,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Evaluation {
    pub winrate: f64,
    pub score: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Comparison {
    /// Each curve is a contiguous evaluated prefix; missing tail values are omitted.
    pub recorded: Vec<Evaluation>,
    pub attempt: Vec<Evaluation>,
    #[serde(default)]
    pub total_plies: usize,
}
impl Comparison {
    pub fn complete(&self) -> bool {
        self.recorded.len() > self.total_plies && self.attempt.len() > self.total_plies
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub passed: bool,
    pub recorded_loss: MoveLoss,
    pub attempt_loss: MoveLoss,
    pub major_mistake: bool,
    #[serde(default)]
    pub comparison: Comparison,
}
impl Report {
    pub fn winrate_saved(&self) -> f64 {
        self.recorded_loss.winrate - self.attempt_loss.winrate
    }
    pub fn score_saved(&self) -> f64 {
        self.recorded_loss.score - self.attempt_loss.score
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
    pub target: usize,
    pub report: Option<Report>,
    pub reveal: Option<(PathBuf, usize)>,
    pub lines: Vec<Vec<Option<Point>>>,
}
struct Problem {
    doc: Document,
    index: usize,
    color: Color,
    key: String,
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
    db: Connection,
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
}
impl Trainer {
    pub fn new(
        reviews: &Path,
        cache: &Path,
        output: &Path,
        profile: EngineProfile,
    ) -> Result<Self> {
        static INSTANCE: AtomicU64 = AtomicU64::new(0);
        let db = crate::review::connection(reviews)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS training_attempts (id INTEGER PRIMARY KEY, problem TEXT NOT NULL, outcome TEXT NOT NULL, report TEXT, document TEXT NOT NULL);")?;
        let review = Review::new(reviews, cache)?;
        Ok(Self {
            state: Snapshot::default(),
            db,
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
        self.seen.insert(self.problem.as_ref().unwrap().key.clone());
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
    fn start_problem(&mut self, doc: Document, index: usize) -> Result<()> {
        self.problem = Some(Problem {
            prefix: prefix(&doc, doc.mainline[index])?,
            color: player_color(&doc, &self.account).unwrap(),
            key: self.problem_key(&doc, index)?,
            index,
            doc: doc.clone(),
        });
        self.reset_attempt();
        self.prefetch_recorded(&doc, index, self.settings.moves * 2)
    }
    pub fn next_request(&mut self) -> Result<Option<Value>> {
        // Grade readiness changes on commands/replies, not each worker poll.
        if self.state.phase == Phase::Searching {
            self.advance()?;
        }
        // Keep one analysis slot available for the next interactive HumanSL reply.
        let reserve = self.state.phase == Phase::YourTurn;
        if self.pending.len() >= crate::engine::ANALYSIS_THREADS - usize::from(reserve) {
            return Ok(None);
        }
        Ok(self.queue.pop_front().map(|job| {
            let q = job.query.clone();
            self.pending.insert(q["id"].as_str().unwrap().into(), job);
            q
        }))
    }
    fn query(&mut self, doc: &Document, node: usize, visits: u64) -> Result<Value> {
        let mut q = serde_json::to_value(doc.position(node)?)?;
        self.sequence += 1;
        q["id"] = json!(format!("training-{}-{}", self.instance, self.sequence));
        q["maxVisits"] = json!(visits);
        q["analyzeTurns"] = json!([q["moves"].as_array().unwrap().len()]);
        Ok(q)
    }
    fn keys(&self, doc: &Document, node: usize, visits: u64) -> Result<(String, String)> {
        let position = doc.position(node)?.analysis_key(&self.profile)?;
        Ok((
            digest(&serde_json::to_vec(&("training-v1", &position, visits))?),
            position,
        ))
    }
    fn need(&mut self, doc: &Document, node: usize, visits: u64, reuse_deeper: bool) -> Result<()> {
        let (key, position) = self.keys(doc, node, visits)?;
        if self.memo.contains_key(&key)
            || self
                .queue
                .iter()
                .chain(self.pending.values())
                .any(|j| matches!(&j.kind, Kind::Evaluation {key:k,..} if *k == key))
        {
            return Ok(());
        }
        let read = |k: &str| -> Result<Option<String>> {
            Ok(self
                .review
                .cache
                .query_row(
                    "SELECT result FROM analysis WHERE key=?1 AND visits>=?2",
                    params![k, visits as i64],
                    |r| r.get(0),
                )
                .optional()?)
        };
        let first = if reuse_deeper { &position } else { &key };
        let second = if reuse_deeper { &key } else { &position };
        let stored = read(first)?;
        let same_budget = if stored.is_some() {
            first == &key
        } else {
            second == &key
        };
        let cached = match stored {
            Some(value) => Some(value),
            None => read(second)?,
        };
        if let Some(value) = cached {
            let a: Analysis = serde_json::from_str(&value)?;
            a.validate()?;
            if reuse_deeper || same_budget || visits == SCREEN_VISITS || a.visits == visits {
                self.memo.insert(key, a);
                return Ok(());
            }
        }
        let query = self.query(doc, node, visits)?;
        self.queue.push_back(Job {
            query,
            kind: Kind::Evaluation {
                key,
                position,
                visits,
            },
        });
        Ok(())
    }
    fn prefetch_recorded(&mut self, doc: &Document, index: usize, plies: usize) -> Result<()> {
        let end = horizon(doc, index, plies).unwrap_or(*doc.mainline.last().unwrap());
        for &node in doc.mainline.iter().skip(index) {
            self.need(doc, node, GRADE_VISITS, false)?;
            if node == end {
                break;
            }
        }
        Ok(())
    }
    fn need_attempt(&mut self, doc: &Document) -> Result<()> {
        self.need(doc, doc.selected, GRADE_VISITS, false)?;
        let (key, _) = self.keys(doc, doc.selected, GRADE_VISITS)?;
        // Newly played moves take priority over queued recorded-line prefetch.
        if let Some(index) = self
            .queue
            .iter()
            .position(|j| matches!(&j.kind, Kind::Evaluation {key:k,..} if *k == key))
        {
            let job = self.queue.remove(index).unwrap();
            self.queue.push_front(job);
        }
        Ok(())
    }
    fn value(&self, doc: &Document, node: usize, visits: u64) -> Result<&Analysis> {
        self.memo
            .get(&self.keys(doc, node, visits)?.0)
            .ok_or_else(|| "Training evaluation is incomplete".into())
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
                let root = &reply["rootInfo"];
                let a = Analysis {
                    visits: root["visits"].as_u64().ok_or("Missing visits")?,
                    winrate: root["winrate"].as_f64().ok_or("Missing winrate")?,
                    score_lead: root["scoreLead"].as_f64().ok_or("Missing score")?,
                    suggestions: vec![],
                    ownership: vec![],
                    ownership_visits: 0,
                };
                a.validate()?;
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
    fn qualifies(
        &self,
        doc: &Document,
        index: usize,
        color: Color,
        visits: u64,
        lead: f64,
        loss: f64,
    ) -> Result<bool> {
        if doc.nodes[doc.mainline[index + 1]]
            .played
            .is_none_or(|m| m.color != color)
            || doc.board(doc.mainline[index])?.next != color
        {
            return Ok(false);
        }
        let a = perspective(self.value(doc, doc.mainline[index], visits)?, color);
        let b = perspective(self.value(doc, doc.mainline[index + 1], visits)?, color);
        Ok(a.0 >= lead && a.1 > 0.0 && a.0 - b.0 >= loss)
    }
    fn problem_key(&self, doc: &Document, index: usize) -> Result<String> {
        Ok(digest(&serde_json::to_vec(&(
            self.account.to_lowercase(),
            doc.source_identity()?,
            doc.position(doc.mainline[index])?,
            &self.profile,
        ))?))
    }
    fn completed(&self, key: &str) -> Result<bool> {
        Ok(self.db.query_row("SELECT EXISTS(SELECT 1 FROM training_attempts WHERE problem=?1 AND outcome IN ('passed','given-up'))",[key],|r|r.get(0))?)
    }
    fn reset_attempt(&mut self) {
        self.report_id = None;
        self.state = Snapshot {
            phase: Phase::YourTurn,
            document: Some(self.problem.as_ref().unwrap().prefix.clone()),
            target: self.settings.moves,
            status: "Your move · play as the player who lost the lead".into(),
            ..Default::default()
        };
    }
    pub fn play(&mut self, point: Option<Point>) -> Result<()> {
        if self.state.phase != Phase::YourTurn {
            return Err("Wait for your turn".into());
        }
        let doc = self.state.document.as_mut().unwrap();
        doc.append_move(point)?;
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
    pub fn reveal(&mut self) -> Result<()> {
        if !matches!(
            self.state.phase,
            Phase::YourTurn | Phase::Opponent | Phase::Grading | Phase::Failed | Phase::Passed
        ) {
            return Err("There is no exercise to reveal".into());
        }
        if self.state.phase != Phase::Passed {
            self.record("given-up")?;
        }
        self.queue.clear();
        self.pending.clear();
        self.state.phase = Phase::Revealing;
        self.state.status = "Preparing suggested lines…".into();
        let p = self.problem.as_ref().unwrap();
        let doc = p.doc.clone();
        let node = doc.mainline[p.index];
        let mut query = self.query(&doc, node, GRADE_VISITS)?;
        query["overrideSettings"] = json!({"analysisPVLen":12});
        self.queue.push_back(Job {
            query,
            kind: Kind::Lines,
        });
        Ok(())
    }
    fn finish_reveal(&mut self, reply: &Value) -> Result<()> {
        let p = self.problem.as_ref().unwrap();
        let path = self.output.join(format!("{}.sgf", p.key));
        let mut doc = p.doc.clone();
        if path.is_file() {
            self.review.import(&path)?;
            let saved = self.review.document().unwrap();
            if saved.position(*saved.mainline.last().unwrap())?
                != doc.position(*doc.mainline.last().unwrap())?
            {
                return Err(
                    "The training review's original game has changed; it was not overwritten"
                        .into(),
                );
            }
            doc = saved.clone();
        }
        let anchor = doc.mainline[p.index];
        let records = self
            .db
            .prepare("SELECT document FROM training_attempts WHERE problem=?1 ORDER BY id")?
            .query_map([&p.key], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for record in records {
            let attempt: Document = serde_json::from_str(&record)?;
            doc.selected = anchor;
            for &id in attempt
                .path(attempt.selected)?
                .iter()
                .skip(p.prefix.nodes.len())
            {
                doc.append_move(attempt.nodes[id].played.unwrap().point)?;
            }
        }
        let infos = reply["moveInfos"]
            .as_array()
            .ok_or("KataGo returned no suggested lines")?;
        let mut candidates = infos
            .iter()
            .filter_map(|m| {
                Some((
                    m,
                    m["visits"].as_u64()?,
                    m["winrate"].as_f64()?,
                    m["scoreLead"].as_f64()?,
                ))
            })
            .filter(|(_, visits, wr, score)| {
                *visits >= 16 && wr.is_finite() && (0.0..=1.0).contains(wr) && score.is_finite()
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, visits, _, _)| std::cmp::Reverse(*visits));
        let Some((_, _, wr, score)) = candidates.first().copied() else {
            return Err("KataGo has not searched a suggested line deeply enough".into());
        };
        let sign = if p.color == Color::Black { 1.0 } else { -1.0 };
        let mut count = 0;
        let mut lines = Vec::new();
        for (m, _, w, s) in candidates {
            if (wr - w) * sign > 0.05 || (score - s) * sign > 2.0 {
                continue;
            }
            let Some(pv) = m["pv"].as_array().filter(|pv| pv.len() >= 2) else {
                continue;
            };
            let mut line = doc.clone();
            line.selected = anchor;
            let mut valid = true;
            for value in pv.iter().take(12) {
                let step = value
                    .as_str()
                    .ok_or("Invalid PV move")
                    .and_then(|s| Point::from_gtp(s, line.size).map_err(|_| "Invalid PV move"));
                if step
                    .and_then(|point| line.append_move(point).map_err(|_| "Illegal PV"))
                    .is_err()
                {
                    valid = false;
                    break;
                }
            }
            if valid {
                let points = pv
                    .iter()
                    .take(12)
                    .map(|v| Point::from_gtp(v.as_str().unwrap(), line.size))
                    .collect::<Result<Vec<_>>>()?;
                let text = points
                    .iter()
                    .map(|p| p.map(|p| p.gtp(line.size)).unwrap_or("pass".into()))
                    .collect::<Vec<_>>()
                    .join(" ");
                let comment = line.nodes[anchor].property("C").unwrap_or("").to_owned();
                line.nodes[anchor].props.insert(
                    "C".into(),
                    vec![format!("{comment}\nSuggested line {}: {text}", count + 1)],
                );
                doc = line;
                lines.push(points);
                count += 1;
                if count == 3 {
                    break;
                }
            }
        }
        if count == 0 {
            return Err(
                "KataGo returned no legal multi-move suggestions; try revealing again".into(),
            );
        }
        std::fs::create_dir_all(&self.output)?;
        let temp = path.with_extension("sgf.tmp");
        std::fs::write(&temp, doc.to_sgf())?;
        std::fs::rename(temp, &path)?;
        self.state.lines = lines;
        self.state.reveal = Some((path, p.index));
        self.state.phase = Phase::Revealed;
        self.state.status = format!("{count} suggested lines · original game remains on top");
        Ok(())
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
fn own_moves(
    doc: &Document,
    start: usize,
    end: usize,
    color: Color,
) -> Result<Vec<(usize, usize)>> {
    let path = doc.path(end)?;
    let first = path
        .iter()
        .position(|id| *id == start)
        .ok_or("The attempt is not descended from its starting position")?;
    Ok(path[first..]
        .windows(2)
        .filter(|pair| doc.nodes[pair[1]].played.is_some_and(|m| m.color == color))
        .map(|pair| (pair[0], pair[1]))
        .collect())
}
fn perspective(a: &Analysis, color: Color) -> (f64, f64) {
    if color == Color::Black {
        (a.winrate, a.score_lead)
    } else {
        (1.0 - a.winrate, -a.score_lead)
    }
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
fn two_passes(doc: &Document) -> bool {
    let n = &doc.nodes[doc.selected];
    n.played.is_some_and(|m| m.point.is_none())
        && n.parent
            .is_some_and(|id| doc.nodes[id].played.is_some_and(|m| m.point.is_none()))
}
fn sample_policy(doc: &Document, policy: &Value, sample: f64) -> Result<Option<Point>> {
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
