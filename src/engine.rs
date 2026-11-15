use crate::{EngineProfile, Result, digest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};
pub(crate) const DEFAULT_PARALLEL_POSITIONS: usize = 8;
fn configuration(settings: &AnalysisSettings) -> String {
    let positions = settings.parallel_positions;
    let threads = settings.threads_per_position;
    let batch = positions * threads;
    format!(
        "logToStderr = true\nreportAnalysisWinratesAs = BLACK\nnumAnalysisThreads = {positions}\nnumSearchThreadsPerAnalysisThread = {threads}\nnnMaxBatchSize = {batch}\nnnCacheSizePowerOfTwo = 18\nnnMutexPoolSizePowerOfTwo = 14\nnumNNServerThreadsPerModel = 1\nmetalDeviceToUseThread0 = 0\nmaxVisits = 64\nanalysisPVLen = 1\n"
    )
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisSettings {
    pub max_visits: Option<u64>,
    pub parallel_positions: usize,
    pub threads_per_position: usize,
}
impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            max_visits: None,
            parallel_positions: DEFAULT_PARALLEL_POSITIONS,
            threads_per_position: 1,
        }
    }
}
impl AnalysisSettings {
    pub fn validate(&self) -> Result<()> {
        if self
            .max_visits
            .is_some_and(|v| !(64..=1_048_576).contains(&v))
        {
            return Err(
                "Visits per position must be between 64 and 1,048,576, or unlimited".into(),
            );
        }
        if !(1..=32).contains(&self.parallel_positions) {
            return Err("Parallel positions must be between 1 and 32".into());
        }
        if !(1..=16).contains(&self.threads_per_position) {
            return Err("Threads per position must be between 1 and 16".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineConfig {
    pub executable: PathBuf,
    pub model: PathBuf,
    #[serde(default)]
    pub analysis: AnalysisSettings,
}
impl EngineConfig {
    pub fn discover(home: &Path) -> Self {
        let executable = [
            PathBuf::from("/opt/homebrew/bin/katago"),
            PathBuf::from("/usr/local/bin/katago"),
        ]
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_default();
        let preferred = home.join("katrain/katrain/models/b10c384h6nbttflrs.bin.gz");
        let model = if preferred.exists() {
            preferred
        } else {
            PathBuf::new()
        };
        Self {
            executable,
            model,
            analysis: AnalysisSettings::default(),
        }
    }
}
#[derive(Debug)]
pub enum EngineEvent {
    Response(Value),
    Diagnostic(String),
    Fault(String),
    Exited,
}
pub struct Engine {
    child: Child,
    stdin: ChildStdin,
    events: Receiver<EngineEvent>,
    config_path: PathBuf,
}
impl Engine {
    pub fn spawn_with_human(config: &EngineConfig, human: &Path) -> Result<Self> {
        if !human.is_file() {
            return Err("HumanSL model is missing".into());
        }
        Self::spawn_models(config, Some(human))
    }
    pub fn spawn(config: &EngineConfig) -> Result<Self> {
        Self::spawn_models(config, None)
    }
    fn spawn_models(config: &EngineConfig, human: Option<&Path>) -> Result<Self> {
        config.analysis.validate()?;
        if !config.model.is_file() {
            return Err("Choose a KataGo .bin.gz model in Engine settings".into());
        }
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let config_path = std::env::temp_dir().join(format!(
            "katastro-{}-{}.cfg",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&config_path, configuration(&config.analysis))?;
        let mut command = Command::new(&config.executable);
        command.arg("analysis");
        if let Some(human) = human {
            command.arg("-human-model").arg(human);
        }
        let spawned = command
            .arg("-config")
            .arg(&config_path)
            .arg("-model")
            .arg(&config.model)
            .arg("-quit-without-waiting")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&config_path);
                return Err(format!("Could not start KataGo: {error}").into());
            }
        };
        let stdin = child.stdin.take().ok_or("KataGo stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("KataGo stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("KataGo stderr unavailable")?;
        let (tx, events) = mpsc::channel();
        let out = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        let event = match serde_json::from_str(&line) {
                            Ok(value) => EngineEvent::Response(value),
                            Err(error) => {
                                EngineEvent::Fault(format!("Invalid KataGo JSON: {error}"))
                            }
                        };
                        if out.send(event).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = out.send(EngineEvent::Fault(error.to_string()));
                        break;
                    }
                }
            }
            let _ = out.send(EngineEvent::Exited);
        });
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else {
                    break;
                };
                if tx.send(EngineEvent::Diagnostic(line)).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            events,
            config_path,
        })
    }
    pub fn send(&mut self, request: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.stdin, request)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }
    pub fn recv_timeout(&self, timeout: Duration) -> Result<EngineEvent> {
        Ok(self.events.recv_timeout(timeout)?)
    }
    pub fn try_recv(&self) -> Option<EngineEvent> {
        self.events.try_recv().ok()
    }
    pub fn profile(config: &EngineConfig) -> Result<EngineProfile> {
        config.analysis.validate()?;
        Ok(EngineProfile {
            model_sha256: digest(&fs::read(&config.model)?),
            engine_sha256: digest(&fs::read(&config.executable)?),
            // Effort and concurrency do not change evaluation semantics. Keep the
            // historical identity so tuning them reuses completed compatible results.
            settings_digest: digest(configuration(&AnalysisSettings::default()).as_bytes()),
        })
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.config_path);
    }
}
