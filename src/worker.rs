use crate::updates::Wake;
use crate::{
    Analysis, Board, Document, EngineProfile, NodeId, Point, Result, Review,
    engine::{Engine, EngineConfig, EngineEvent},
    scheduler::Scheduler,
};
use serde_json::json;
mod discovery;
mod snapshots;
pub use snapshots::Snapshots;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub enum Command {
    Open(PathBuf),
    OpenAndAnalyze(PathBuf),
    Select(NodeId),
    NumberFrom(Option<NodeId>),
    Play(Option<Point>),
    Step(bool),
    First,
    Last,
    Export(PathBuf),
    Start,
    Pause,
    Configure(EngineConfig),
    SaveGameStatistics(crate::archive::StatisticsSettings),
    PrepareTrainingLibraries(Vec<(String, Vec<PathBuf>)>),
    PrepareTraining {
        games: Vec<PathBuf>,
        account: String,
    },
    Shutdown,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub document: Option<Document>,
    pub board: Option<Board>,
    pub move_numbers: Vec<Option<usize>>,
    pub values: BTreeMap<NodeId, Analysis>,
    pub source: Option<PathBuf>,
    pub config: EngineConfig,
    pub game_statistics: Option<crate::archive::StatisticsSettings>,
    pub status: String,
    pub error: Option<String>,
    pub diagnostic: Option<String>,
    pub running: bool,
    pub preparing_training: bool,
    pub training_indexed: bool,
    pub coverage: (usize, usize),
    pub analysis_target: Option<u64>,
}
impl Snapshot {
    pub fn empty(config: EngineConfig) -> Self {
        Self {
            config,
            status: "Open an SGF to begin".into(),
            ..Default::default()
        }
    }
}
pub struct Client {
    pub commands: Sender<Command>,
    pub snapshots: Snapshots,
    thread: Option<JoinHandle<()>>,
}
impl Client {
    pub fn spawn(reviews: PathBuf, cache: PathBuf, config: EngineConfig) -> Self {
        Self::spawn_with_wake(reviews, cache, config, None)
    }
    pub fn spawn_with_wake(
        reviews: PathBuf,
        cache: PathBuf,
        config: EngineConfig,
        wake: Wake,
    ) -> Self {
        let (commands, rx) = mpsc::channel();
        let (publish, snapshots) = Snapshots::channel(wake);
        let thread = thread::spawn(move || match Worker::new(reviews, cache, config.clone()) {
            Ok(mut worker) => worker.run(rx, publish),
            Err(error) => {
                let mut state = Snapshot::empty(config);
                state.error = Some(error.to_string());
                state.status = "Could not open local storage".into();
                publish(state);
            }
        });
        Self {
            commands,
            snapshots,
            thread: Some(thread),
        }
    }
    pub fn send(&self, command: Command) -> Result<()> {
        Ok(self.commands.send(command)?)
    }
    pub fn recv(&self, timeout: Duration) -> Result<Snapshot> {
        Ok(self.snapshots.recv_timeout(timeout)?)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct Worker {
    review: Review,
    state: Snapshot,
    engine: Option<(EngineProfile, Engine)>,
    values_profile: Option<EngineProfile>,
    scheduler: Option<Scheduler>,
    generation: u64,
    number_from: Option<NodeId>,
    unsupported_mainline: bool,
    discovery: discovery::Discovery,
}
fn ensure_engine<'a>(
    slot: &'a mut Option<(EngineProfile, Engine)>,
    config: &EngineConfig,
    profile: &EngineProfile,
) -> Result<&'a mut Engine> {
    if slot.as_ref().is_none_or(|(loaded, _)| loaded != profile) {
        *slot = Some((profile.clone(), Engine::spawn(config)?));
    }
    Ok(&mut slot.as_mut().unwrap().1)
}
impl Worker {
    fn new(reviews: PathBuf, cache: PathBuf, config: EngineConfig) -> Result<Self> {
        let review = Review::new(&reviews, &cache)?;
        let config = if let Some(mut saved) = review.setting::<EngineConfig>("engine")? {
            if saved.executable.as_os_str().is_empty() {
                saved.executable = config.executable;
            }
            if saved.model.as_os_str().is_empty() {
                saved.model = config.model;
            }
            saved
        } else {
            config
        };
        let stats = match review.setting::<crate::archive::StatisticsSettings>("game_statistics") {
            Ok(settings) => settings,
            Err(error) if error.is::<serde_json::Error>() => None,
            Err(error) => return Err(error),
        };
        let state = Snapshot {
            game_statistics: Some(stats.filter(|s| s.is_valid()).unwrap_or_default()),
            ..Snapshot::empty(config)
        };
        Ok(Self {
            review,
            state,
            engine: None,
            values_profile: None,
            scheduler: None,
            generation: 0,
            number_from: None,
            unsupported_mainline: false,
            discovery: discovery::Discovery::new(reviews, cache),
        })
    }
    fn publish(&self, publish: &impl Fn(Snapshot)) {
        let mut snapshot = self.state.clone();
        snapshot.document = self.review.document().cloned();
        snapshot.source = self.review.source().map(PathBuf::from);
        if let Some(scheduler) = &self.scheduler {
            snapshot.values = scheduler.values.clone();
        }
        if let Some(doc) = &snapshot.document {
            if let Ok((board, numbers)) = match self.number_from {
                Some(anchor) => doc.numbered_board(doc.selected, anchor),
                None => doc.board(doc.selected).map(|board| (board, Vec::new())),
            } {
                snapshot.board = Some(board);
                snapshot.move_numbers = numbers;
            }
            snapshot.coverage = (
                doc.mainline
                    .iter()
                    .filter(|id| snapshot.values.contains_key(id))
                    .count(),
                doc.mainline.len(),
            );
        }
        publish(snapshot);
    }
    fn pause(&mut self) {
        self.discovery.pause();
        self.state.preparing_training = false;
        if let Some(scheduler) = self.scheduler.take() {
            self.state.values = scheduler.values;
        }
        if let Some((_, engine)) = self.engine.as_mut() {
            let _ = engine
                .send(&json!({"id":format!("pause-{}",self.generation),"action":"terminate_all"}));
        }
        self.state.running = false;
        self.state.status = "Analysis paused · variations autosave".into();
    }
    fn start(&mut self) -> Result<()> {
        let doc = self.review.document().cloned().ok_or("Open an SGF first")?;
        let profile = Engine::profile(&self.state.config)?;
        let mut values = self.review.cached_analysis(&profile)?;
        self.discovery.started(profile.clone(), &values);
        if self.values_profile.as_ref() == Some(&profile) {
            let previous = self
                .scheduler
                .as_ref()
                .map_or(&self.state.values, |s| &s.values);
            for (node, value) in previous {
                let combined = values
                    .get(node)
                    .map_or_else(|| value.clone(), |old| old.merge(value));
                values.insert(*node, combined);
            }
        }
        ensure_engine(&mut self.engine, &self.state.config, &profile)?;
        self.review.save_profile(&profile)?;
        self.values_profile = Some(profile.clone());
        self.generation += 1;
        let mut scheduler = Scheduler::new(self.generation, doc, profile, values)
            .with_settings(&self.state.config.analysis)?;
        self.unsupported_mainline = scheduler.unsupported();
        // The scheduler owns the live results until pause moves them back.
        self.state.values.clear();
        self.scheduler = Some(scheduler);
        self.state.running = true;
        self.state.status = "Analyzing · quick chart first, then refinement".into();
        Ok(())
    }
    fn changed_selection(&mut self) -> Result<()> {
        if let Some(scheduler) = self.scheduler.as_mut() {
            let doc = self.review.document().cloned().ok_or("Open an SGF first")?;
            let canceled = scheduler.select(doc);
            self.state.running = true;
            if let Some((_, engine)) = self.engine.as_mut() {
                for canceled in canceled {
                    engine.send(
                        &json!({"id":format!("cancel-{}",canceled.id),"action":"terminate",
                        "terminateId":canceled.id,"turnNumbers":canceled.turns}),
                    )?;
                }
            }
        }
        Ok(())
    }
    fn command(&mut self, command: Command) -> Result<()> {
        self.state.error = None;
        match command {
            Command::Open(path) | Command::OpenAndAnalyze(path) => {
                // Validate import before stopping or replacing a working review.
                self.review.import(&path)?;
                self.number_from = None;
                self.pause();
                self.discovery.opened();
                self.state.training_indexed = false;
                self.generation += 1;
                self.values_profile = self.review.last_profile()?;
                self.state.values = if let Some(profile) = &self.values_profile {
                    self.review.cached_analysis(profile)?
                } else {
                    BTreeMap::new()
                };
                self.state.analysis_target = None;
                self.state.status = "Review ready · saved variations restored".into();
            }
            Command::Select(id) => {
                self.review.select(id)?;
                self.changed_selection()?;
            }
            Command::NumberFrom(anchor) => {
                if let Some(anchor) = anchor {
                    self.review
                        .document()
                        .ok_or("Open an SGF first")?
                        .path(anchor)?;
                }
                self.number_from = anchor;
            }
            Command::Play(point) => {
                self.review.play(point)?;
                self.changed_selection()?;
            }
            Command::Step(forward) => {
                let doc = self.review.document().ok_or("Open an SGF first")?;
                let node = &doc.nodes[doc.selected];
                let next = if forward {
                    doc.continuation(doc.selected)
                } else {
                    node.parent
                };
                if let Some(next) = next {
                    self.review.select(next)?;
                    self.changed_selection()?;
                }
            }
            Command::First => {
                self.review.select(0)?;
                self.changed_selection()?;
            }
            Command::Last => {
                let last = *self
                    .review
                    .document()
                    .ok_or("Open an SGF first")?
                    .mainline
                    .last()
                    .ok_or("Empty game")?;
                self.review.select(last)?;
                self.changed_selection()?;
            }
            Command::Export(path) => {
                self.review.export(&path)?;
                self.state.status = format!("Exported {}", path.display());
            }
            Command::Start => {
                if self.review.document().is_none() {
                    self.discovery.resume();
                } else if !self.state.running {
                    self.start()?;
                }
            }
            Command::Pause => self.pause(),
            Command::SaveGameStatistics(settings) => {
                if !settings.is_valid() {
                    return Err("Choose a valid date range and positive point thresholds".into());
                }
                self.review.save_setting("game_statistics", &settings)?;
                self.state.game_statistics = Some(settings);
            }
            Command::Configure(config) => {
                config.analysis.validate()?;
                let mut saved = config.clone();
                if let Some(runtime) = crate::runtime::directory() {
                    if saved.executable == runtime.join("Helpers/katago") {
                        saved.executable.clear();
                    }
                    if saved.model == runtime.join("Resources/Models/review.bin.gz") {
                        saved.model.clear();
                    }
                }
                self.review.save_setting("engine", &saved)?;
                self.pause();
                self.discovery.reconfigure();
                self.state.training_indexed = false;
                self.engine = None;
                self.state.config = config;
                self.state.analysis_target = None;
                self.state.status =
                    "Engine settings saved · cached review remains available".into();
            }
            Command::Shutdown => {}
            Command::PrepareTraining { games, account } => {
                self.discovery.libraries(vec![(account, games)])
            }
            Command::PrepareTrainingLibraries(libraries) => {
                self.discovery.libraries(libraries);
            }
        }
        Ok(())
    }
    fn poll_engine(&mut self) -> bool {
        let mut dirty = false;
        for _ in 0..256 {
            let event = self.engine.as_ref().and_then(|(_, e)| e.try_recv());
            let Some(event) = event else {
                break;
            };
            dirty = true;
            match event {
                EngineEvent::Response(reply) => {
                    if let Some(trainer) = &mut self.discovery.trainer
                        && let Err(error) = trainer.accept(&reply, 0.5)
                    {
                        self.pause();
                        self.state.error = Some(error.to_string());
                    }
                    if let Some(scheduler) = self.scheduler.as_mut() {
                        match scheduler.accept(&reply) {
                            Ok(results) => {
                                for (target, value) in results
                                    .into_iter()
                                    .filter(|_| reply["isDuringSearch"] == false)
                                {
                                    if let Err(error) =
                                        self.review.store_analysis(&target.key, &value)
                                    {
                                        self.state.error = Some(error.to_string());
                                    } else {
                                        self.discovery.completed(target.node, value.visits);
                                    }
                                }
                            }
                            Err(error) => {
                                self.pause();
                                self.engine = None;
                                self.state.error = Some(error.to_string());
                            }
                        }
                    }
                }
                EngineEvent::Diagnostic(line) => {
                    self.state.diagnostic = Some(line);
                }
                EngineEvent::Fault(error) => {
                    self.pause();
                    self.engine = None;
                    self.state.error = Some(error);
                    break;
                }
                EngineEvent::Exited => {
                    self.pause();
                    self.engine = None;
                    self.state.error=Some("KataGo exited. Saved reviews and analysis are still available; use Analyze to restart.".into());
                    break;
                }
            }
        }
        dirty
    }
    fn fail(&mut self, error: impl std::fmt::Display) {
        self.pause();
        self.state.error = Some(error.to_string());
    }
    fn refine(&mut self) -> bool {
        if !self.state.running {
            return false;
        }
        let mut dirty = false;
        let result = (|| -> Result<()> {
            // Fill available positions after each completion, not one per poll.
            while let Some(scheduler) = self.scheduler.as_mut() {
                let Some(request) = scheduler.next_request()? else {
                    break;
                };
                self.state.analysis_target = Some(request.visits);
                self.state.status = format!(
                    "Refining · target {} visits per position · variations autosave",
                    request.visits,
                );
                if self.unsupported_mainline {
                    self.state
                        .status
                        .push_str(" · analysis unavailable after setup changes");
                }
                if let Some((_, engine)) = self.engine.as_mut()
                    && let Err(error) = engine.send(&request.query)
                {
                    self.engine = None;
                    return Err(error);
                }
                dirty = true;
            }
            if let Some(scheduler) = self.scheduler.as_ref().filter(|s| s.is_complete()) {
                self.state.running = false;
                let (covered, total) = scheduler.coverage();
                self.state.status = if covered == total {
                    self.state.config.analysis.max_visits.map_or_else(
                        || "Analysis finished · maximum search effort reached".into(),
                        |limit| format!("Analysis finished · {limit} visits per position reached"),
                    )
                } else {
                    format!(
                        "Analysis finished · {covered} / {total} played positions evaluated; analysis unavailable after setup changes or engine errors"
                    )
                };
                dirty = true;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(error);
            dirty = true;
        }
        dirty
    }
    fn prepare_training(&mut self) -> bool {
        let mut dirty = false;
        let discovery = (|| -> Result<()> {
            if let (Some(source), Some(doc)) = (self.review.source(), self.review.document()) {
                self.discovery.index_review(source, doc)?;
                let indexed = self.discovery.indexed();
                dirty |= self.state.training_indexed != indexed;
                self.state.training_indexed = indexed;
            } else {
                self.discovery.start_idle(&self.state.config)?;
                if let Some(trainer) = &mut self.discovery.trainer {
                    while let Some(query) = trainer.next_request()? {
                        ensure_engine(&mut self.engine, &self.state.config, trainer.profile())?
                            .send(&query)?;
                    }
                    let snapshot = trainer.snapshot();
                    self.state.preparing_training = snapshot.phase
                        == crate::training::Phase::Searching
                        || self.discovery.pending();
                    dirty |= self.state.status != snapshot.status;
                    self.state.status = snapshot.status;
                }
            }
            Ok(())
        })();
        if let Err(error) = discovery {
            self.fail(error);
            dirty = true;
        }
        dirty
    }
    fn run(&mut self, commands: Receiver<Command>, snapshots: impl Fn(Snapshot)) {
        self.publish(&snapshots);
        let mut last_publish = Instant::now();
        let mut dirty = false;
        loop {
            match commands.recv_timeout(Duration::from_millis(12)) {
                Ok(Command::Shutdown) => break,
                Ok(command) => {
                    let (command, auto) = match command {
                        Command::OpenAndAnalyze(path) => (Command::Open(path), true),
                        other => (other, false),
                    };
                    let result = self.command(command);
                    let success = result.is_ok();
                    if let Err(error) = result {
                        self.state.error = Some(error.to_string());
                    }
                    self.publish(&snapshots);
                    if success
                        && auto
                        && self.state.config.executable.is_file()
                        && self.state.config.model.is_file()
                    {
                        if let Err(error) = self.start() {
                            self.state.error = Some(error.to_string());
                        }
                        self.publish(&snapshots);
                    }
                    last_publish = Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            dirty |= self.poll_engine();
            dirty |= self.refine();
            dirty |= self.prepare_training();
            if dirty && last_publish.elapsed() >= Duration::from_millis(30) {
                self.publish(&snapshots);
                dirty = false;
                last_publish = Instant::now();
            }
        }
    }
}
