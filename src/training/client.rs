//! Engine, database, and model download work stays on this worker thread.
use super::{Phase, Settings, Snapshot, Trainer};
use crate::updates::{self, Wake};
use crate::{
    Point, Result, digest_file,
    engine::{Engine, EngineConfig, EngineEvent},
};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};
pub const HUMAN_MODEL: &str = "b18c384nbt-humanv0.bin.gz";
const HUMAN_SHA256: &str = "637746e44f0efe00ad1245a50aa9bbf0716efe364c43965ead97bd6835d84ab5";
pub struct Options {
    pub reviews: PathBuf,
    pub cache: PathBuf,
    pub engine: EngineConfig,
    pub human_model: PathBuf,
    pub model_url: String,
    pub model_sha256: String,
}
impl Options {
    pub fn new(reviews: PathBuf, cache: PathBuf, engine: EngineConfig) -> Self {
        let human_model = crate::runtime::human_model().unwrap_or_else(|| {
            reviews
                .parent()
                .unwrap_or(Path::new("."))
                .join("models")
                .join(HUMAN_MODEL)
        });
        Self {
            reviews,
            cache,
            engine,
            human_model,
            model_url: format!(
                "https://github.com/lightvector/KataGo/releases/download/v1.15.0/{HUMAN_MODEL}"
            ),
            model_sha256: HUMAN_SHA256.into(),
        }
    }
}
#[derive(Clone, Debug)]
pub enum Command {
    Play(Option<Point>),
    Retry,
    Next,
    Reveal,
    Cancel,
}
pub struct Client {
    pub snapshots: Receiver<Snapshot>,
    commands: Sender<Command>,
    canceled: Arc<AtomicBool>,
}
impl Client {
    pub fn spawn(
        options: Options,
        games: Vec<PathBuf>,
        account: String,
        settings: Settings,
        wake: Wake,
    ) -> Self {
        let (commands, rx) = mpsc::channel();
        let (publish, snapshots) = updates::channel(wake);
        let canceled = Arc::new(AtomicBool::new(false));
        let stop = canceled.clone();
        thread::spawn(move || {
            let result = (|| -> Result<()> {
                publish(Snapshot::searching("Preparing training…"));
                install_model(&options, &stop, |status| {
                    publish(Snapshot::searching(status))
                })?;
                if stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let output = options
                    .reviews
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join("Training");
                let mut trainer = Trainer::new(
                    &options.reviews,
                    &options.cache,
                    &output,
                    Engine::profile(&options.engine)?,
                )?
                .with_analysis_settings(&options.engine.analysis)?;
                trainer.begin(games, account, settings)?;
                publish(trainer.snapshot());
                let mut engine: Option<Engine> = None;
                let mut diagnostic = String::new();
                let mut last_response = Instant::now();
                loop {
                    if stop.load(Ordering::Relaxed) {
                        trainer.cancel();
                        publish(trainer.snapshot());
                        return Ok(());
                    }
                    let mut dirty = false;
                    match rx.recv_timeout(Duration::from_millis(12)) {
                        Ok(command) => {
                            trainer.state.error = None;
                            let replace =
                                matches!(command, Command::Retry | Command::Next | Command::Reveal);
                            let result = match command {
                                Command::Play(point) => trainer.play(point),
                                Command::Retry => trainer.retry(),
                                Command::Next => trainer.next_problem(),
                                Command::Reveal => trainer.reveal(),
                                Command::Cancel => {
                                    trainer.cancel();
                                    publish(trainer.snapshot());
                                    return Ok(());
                                }
                            };
                            if result.is_ok()
                                && replace
                                && let Some(engine) = &mut engine
                            {
                                engine.send(&serde_json::json!({"id":"training-cancel","action":"terminate_all"}))?;
                            }
                            if let Err(error) = result {
                                trainer.state.error = Some(error.to_string());
                            }
                            dirty = true;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if let Some(engine) = &engine {
                        while let Some(event) = engine.try_recv() {
                            match event {
                                EngineEvent::Response(reply) => {
                                    let mut bytes = [0; 8];
                                    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
                                    let sample = (u64::from_ne_bytes(bytes) >> 11) as f64
                                        / (1u64 << 53) as f64;
                                    trainer.accept(&reply, sample)?;
                                    last_response = Instant::now();
                                    dirty = true;
                                }
                                EngineEvent::Diagnostic(message) => diagnostic = message,
                                EngineEvent::Fault(error) => return Err(error.into()),
                                EngineEvent::Exited => {
                                    return Err(format!("KataGo stopped: {diagnostic}").into());
                                }
                            }
                        }
                    }
                    while let Some(query) = trainer.next_request()? {
                        if engine.is_none() {
                            // A prepared exercise can wait for player input before starting KataGo.
                            if Engine::profile(&options.engine)? != *trainer.profile() {
                                return Err("Engine or model changed. Restart training to use the new analysis profile.".into());
                            }
                            engine = Some(Engine::spawn_with_human(
                                &options.engine,
                                &options.human_model,
                            )?);
                        }
                        if trainer.pending.len() == 1 {
                            last_response = Instant::now();
                        }
                        engine.as_mut().unwrap().send(&query)?;
                    }
                    if !trainer.pending.is_empty()
                        && last_response.elapsed() > Duration::from_secs(120)
                    {
                        return Err(
                            "KataGo did not finish a training request within two minutes".into(),
                        );
                    }
                    if matches!(trainer.state.phase, Phase::Revealed | Phase::Idle) {
                        engine = None;
                    }
                    if dirty {
                        publish(trainer.snapshot());
                    }
                }
            })();
            if stop.load(Ordering::Relaxed) {
                publish(Snapshot::default());
            } else if let Err(error) = result {
                publish(Snapshot {
                    error: Some(error.to_string()),
                    status: "Training stopped. Return to setup to try again.".into(),
                    ..Default::default()
                });
            }
        });
        Self {
            snapshots,
            commands,
            canceled,
        }
    }
    pub fn send(&self, command: Command) -> Result<()> {
        if matches!(command, Command::Cancel) {
            self.canceled.store(true, Ordering::Relaxed);
        }
        Ok(self.commands.send(command)?)
    }
    pub fn recv(&self, timeout: Duration) -> Result<Snapshot> {
        Ok(self.snapshots.recv_timeout(timeout)?)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::Relaxed);
        let _ = self.commands.send(Command::Cancel); /* Never block the UI waiting for network I/O. */
    }
}
fn install_model(options: &Options, stop: &AtomicBool, progress: impl Fn(String)) -> Result<()> {
    if digest_file(&options.human_model).is_ok_and(|hash| hash == options.model_sha256) {
        return Ok(());
    }
    if crate::runtime::human_model().as_ref() == Some(&options.human_model) {
        return Err(
            "Bundled HumanSL model is missing or damaged. Rebuild or reinstall Katastro.".into(),
        );
    }
    progress("Downloading the 1-dan model · 99 MB, once per installation".into());
    let parent = options.human_model.parent().ok_or("Invalid model path")?;
    std::fs::create_dir_all(parent)?;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let temp = parent.join(format!(
        "human-{}-{}.part",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()?;
        let mut response = client.get(&options.model_url).send()?.error_for_status()?;
        let mut file = std::fs::File::create(&temp)?;
        let mut buffer = [0; 65536];
        let mut count = 0;
        let mut displayed = 0;
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err("Training canceled".into());
            }
            let n = response.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n;
            if count > 110_000_000 {
                return Err("Unexpected HumanSL model size".into());
            }
            file.write_all(&buffer[..n])?;
            if count / 1_000_000 > displayed {
                displayed = count / 1_000_000;
                progress(format!("Downloading the 1-dan model · {displayed} / 99 MB"));
            }
        }
        if digest_file(&temp)? != options.model_sha256 {
            return Err("HumanSL model checksum mismatch; download was not installed".into());
        }
        if stop.load(Ordering::Relaxed) {
            return Err("Training canceled".into());
        }
        file.sync_all()?;
        std::fs::rename(&temp, &options.human_model)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    result
}
