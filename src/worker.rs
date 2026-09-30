use crate::{
    Analysis, Board, Document, NodeId, Point, Result, Review,
    engine::{Engine, EngineConfig, EngineEvent},
    scheduler::Scheduler,
};
use rusqlite::OptionalExtension;
use serde_json::json;
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
    Play(Option<Point>),
    Step(bool),
    First,
    Last,
    Export(PathBuf),
    Start,
    Pause,
    Configure(EngineConfig),
    Shutdown,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub document: Option<Document>,
    pub board: Option<Board>,
    pub values: BTreeMap<NodeId, Analysis>,
    pub source: Option<PathBuf>,
    pub config: EngineConfig,
    pub status: String,
    pub error: Option<String>,
    pub diagnostic: Option<String>,
    pub running: bool,
    pub coverage: (usize, usize),
}
impl Snapshot {
    pub fn empty(config: EngineConfig) -> Self {
        Self {
            document: None,
            board: None,
            values: BTreeMap::new(),
            source: None,
            config,
            status: "Open an SGF to begin".into(),
            error: None,
            diagnostic: None,
            running: false,
            coverage: (0, 0),
        }
    }
}
pub struct Client {
    pub commands: Sender<Command>,
    pub snapshots: Receiver<Snapshot>,
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
        wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let (commands, rx) = mpsc::channel();
        let (tx, snapshots) = mpsc::channel();
        let thread = thread::spawn(move || match Worker::new(reviews, cache, config.clone()) {
            Ok(mut worker) => worker.run(rx, tx, wake),
            Err(error) => {
                let mut state = Snapshot::empty(config);
                state.error = Some(error.to_string());
                state.status = "Could not open local storage".into();
                let _ = tx.send(state);
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
    engine: Option<Engine>,
    scheduler: Option<Scheduler>,
    generation: u64,
}
impl Worker {
    fn new(reviews: PathBuf, cache: PathBuf, config: EngineConfig) -> Result<Self> {
        let review = Review::new(&reviews, &cache)?;
        let saved: Option<String> = review
            .reviews
            .query_row("SELECT value FROM settings WHERE key='engine'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let config = if let Some(saved) = saved {
            serde_json::from_str(&saved)?
        } else {
            config
        };
        Ok(Self {
            review,
            state: Snapshot::empty(config),
            engine: None,
            scheduler: None,
            generation: 0,
        })
    }
    fn publish(&mut self, tx: &Sender<Snapshot>) {
        self.state.document = self.review.document().cloned();
        self.state.source = self.review.source().map(PathBuf::from);
        self.state.board = self
            .review
            .document()
            .and_then(|doc| doc.board(doc.selected).ok());
        if let Some(scheduler) = &self.scheduler {
            self.state.values = scheduler.values.clone();
            self.state.coverage = scheduler.coverage();
        } else if let Some(doc) = self.review.document() {
            self.state.coverage = (
                doc.mainline
                    .iter()
                    .filter(|id| self.state.values.contains_key(id))
                    .count(),
                doc.mainline.len(),
            );
        }
        let _ = tx.send(self.state.clone());
    }
    fn pause(&mut self) {
        if let Some(scheduler) = self.scheduler.take() {
            self.state.values = scheduler.values;
        }
        if let Some(engine) = self.engine.as_mut() {
            let _ = engine
                .send(&json!({"id":format!("pause-{}",self.generation),"action":"terminate_all"}));
        }
        self.state.running = false;
        self.state.status = "Analysis paused · variations autosave".into();
    }
    fn start(&mut self) -> Result<()> {
        let doc = self.review.document().cloned().ok_or("Open an SGF first")?;
        let profile = Engine::profile(&self.state.config)?;
        let values = self.review.cached_analysis(&profile)?;
        if self.engine.is_none() {
            self.engine = Some(Engine::spawn(&self.state.config)?);
        }
        self.review.save_profile(&profile)?;
        self.generation += 1;
        let mut scheduler = Scheduler::new(self.generation, doc, profile, values);
        scheduler.unsupported();
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
            if let Some(engine) = self.engine.as_mut() {
                for id in canceled {
                    engine.send(
                        &json!({"id":format!("cancel-{id}"),"action":"terminate","terminateId":id}),
                    )?;
                }
            }
        }
        Ok(())
    }
    fn command(&mut self, command: Command) -> Result<()> {
        self.state.error = None;
        match command {
            Command::OpenAndAnalyze(path) => {
                self.command(Command::Open(path))?;
                self.start()?;
            }
            Command::Open(path) => {
                // Validate import before stopping or replacing a working review.
                self.review.import(&path)?;
                self.pause();
                self.generation += 1;
                self.state.values = if let Some(profile) = self.review.last_profile()? {
                    self.review.cached_analysis(&profile)?
                } else {
                    BTreeMap::new()
                };
                self.state.status = "Review ready · saved variations restored".into();
            }
            Command::Select(id) => {
                self.review.select(id)?;
                self.changed_selection()?;
            }
            Command::Play(point) => {
                self.review.play(point)?;
                self.changed_selection()?;
            }
            Command::Step(forward) => {
                let doc = self.review.document().ok_or("Open an SGF first")?;
                let node = &doc.nodes[doc.selected];
                let next = if forward {
                    node.children.first().copied()
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
                if !self.state.running {
                    self.start()?;
                }
            }
            Command::Pause => self.pause(),
            Command::Configure(config) => {
                self.pause();
                self.engine = None;
                self.review.reviews.execute("INSERT INTO settings(key,value) VALUES ('engine',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&config)?])?;
                self.state.config = config;
                self.state.status =
                    "Engine settings saved · cached review remains available".into();
            }
            Command::Shutdown => {}
        }
        Ok(())
    }
    fn run(
        &mut self,
        commands: Receiver<Command>,
        snapshots: Sender<Snapshot>,
        wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) {
        self.publish(&snapshots);
        if let Some(wake) = &wake {
            wake();
        }
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
                    if let Some(wake) = &wake {
                        wake();
                    }
                    if success
                        && auto
                        && self.state.config.executable.is_file()
                        && self.state.config.model.is_file()
                    {
                        if let Err(error) = self.start() {
                            self.state.error = Some(error.to_string());
                        }
                        self.publish(&snapshots);
                        if let Some(wake) = &wake {
                            wake();
                        }
                    }
                    last_publish = Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            for _ in 0..256 {
                let event = self.engine.as_ref().and_then(Engine::try_recv);
                let Some(event) = event else {
                    break;
                };
                match event {
                    EngineEvent::Response(reply) => {
                        if let Some(scheduler) = self.scheduler.as_mut() {
                            match scheduler.accept(&reply) {
                                Ok(results) => {
                                    for (target, value) in results {
                                        if let Err(error) =
                                            self.review.store_analysis(&target.key, &value)
                                        {
                                            self.state.error = Some(error.to_string());
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
                        dirty = true;
                    }
                    EngineEvent::Diagnostic(line) => {
                        self.state.diagnostic = Some(line);
                        dirty = true;
                    }
                    EngineEvent::Fault(error) => {
                        self.pause();
                        self.engine = None;
                        self.state.error = Some(error);
                        dirty = true;
                        break;
                    }
                    EngineEvent::Exited => {
                        self.pause();
                        self.engine = None;
                        self.state.error=Some("KataGo exited. Saved reviews and analysis are still available; use Analyze to restart.".into());
                        dirty = true;
                        break;
                    }
                }
            }
            if self.state.running {
                let planned = self.scheduler.as_mut().map(Scheduler::next_request);
                match planned {
                    Some(Ok(Some(request))) => {
                        if let Some(engine) = self.engine.as_mut()
                            && let Err(error) = engine.send(&request.query)
                        {
                            self.pause();
                            self.engine = None;
                            self.state.error = Some(error.to_string());
                        }
                        dirty = true;
                    }
                    Some(Err(error)) => {
                        self.pause();
                        self.state.error = Some(error.to_string());
                        dirty = true;
                    }
                    _ => {}
                }
                if self.scheduler.as_ref().is_some_and(Scheduler::is_complete) {
                    self.state.running = false;
                    let (covered, total) = self.scheduler.as_ref().unwrap().coverage();
                    self.state.status = if covered == total {
                        "Analysis complete · 64 visits per played position".into()
                    } else {
                        format!(
                            "Analysis finished · {covered} / {total} played positions evaluated; analysis unavailable after setup changes or engine errors"
                        )
                    };
                    dirty = true;
                }
            }
            if dirty && last_publish.elapsed() >= Duration::from_millis(30) {
                self.publish(&snapshots);
                if let Some(wake) = &wake {
                    wake();
                }
                dirty = false;
                last_publish = Instant::now();
            }
        }
    }
}
