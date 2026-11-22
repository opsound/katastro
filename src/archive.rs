//! Game archive synchronization, independent of the desktop shell.
use crate::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use zeroize::Zeroizing;
pub mod categories;
mod fox;
mod http;
mod ogs;
mod pandanet;
pub mod period;
pub mod play_times;
pub mod rank_history;
mod remote;
mod storage;
mod sync;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Server {
    #[default]
    Pandanet,
    Fox,
    Ogs,
}
impl Server {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pandanet => "Pandanet",
            Self::Fox => "Fox",
            Self::Ogs => "OGS",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct StatisticsSettings {
    pub period: period::Period,
    pub compare: bool,
    pub thresholds: categories::Thresholds,
}
impl Default for StatisticsSettings {
    fn default() -> Self {
        Self {
            period: Default::default(),
            compare: true,
            thresholds: Default::default(),
        }
    }
}
impl StatisticsSettings {
    pub(crate) fn is_valid(self) -> bool {
        [self.thresholds.comeback, self.thresholds.blunder]
            .into_iter()
            .all(|v| v.is_finite() && v > 0.0)
            && !matches!(self.period, period::Period::Custom(range) if range.start > range.end)
    }
}

pub const ENDPOINT: &str = "https://my.pandanet.co.jp/cgi-bin/cgi.exe?MH";
pub const SYNC_INTERVAL: u64 = 15 * 60;

pub trait Credentials: Send + Sync {
    fn load(&self, account: &str) -> Result<Option<String>>;
    fn save(&self, account: &str, password: &str) -> Result<()>;
    fn forget(&self, account: &str) -> Result<()>;
}
#[derive(Clone, Debug)]
pub struct Game {
    pub id: String,
    pub date: String,
    /// Unix seconds for ordering. Date-only records use a time on the recorded local day.
    pub timestamp: Option<i64>,
    pub white: String,
    pub black: String,
    pub result: String,
    pub path: PathBuf,
}
#[derive(Clone, Debug)]
pub struct FailedGame {
    pub id: String,
    pub date: String,
    pub message: String,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub server: Server,
    pub limit: Option<usize>,
    pub failures: Vec<FailedGame>,
    pub username: String,
    pub remember: bool,
    pub automatic: bool,
    pub games: Vec<Game>,
    pub rank_history: Arc<[rank_history::RankPoint]>,
    pub status: String,
    pub error: Option<String>,
    pub syncing: bool,
    pub page: usize,
    pub pages: usize,
    pub downloaded: usize,
    pub last_sync: Option<u64>,
}
pub enum Command {
    Connect {
        username: String,
        password: String,
        remember: bool,
    },
    SyncNow,
    SetLimit(Option<usize>),
    SetAutomatic(bool),
    Disconnect,
    Tick,
}
#[derive(Default)]
pub struct Cancellation {
    stopped: Mutex<bool>,
    wake: Condvar,
}
impl Cancellation {
    pub fn cancel(&self) {
        *self.stopped.lock().unwrap() = true;
        self.wake.notify_all();
    }
    pub fn is_canceled(&self) -> bool {
        *self.stopped.lock().unwrap()
    }
    fn check(&self) -> Result<()> {
        if self.is_canceled() {
            Err("Sync canceled".into())
        } else {
            Ok(())
        }
    }
    fn wait(&self, duration: Duration) -> Result<()> {
        let guard = self.stopped.lock().unwrap();
        let (guard, _) = self
            .wake
            .wait_timeout_while(guard, duration, |stopped| !*stopped)
            .unwrap();
        if *guard {
            Err("Sync canceled".into())
        } else {
            Ok(())
        }
    }
}
pub struct Service {
    state: Snapshot,
    root: PathBuf,
    db: Connection,
    remote: remote::Remote,
    credentials: Arc<dyn Credentials>,
    password: Option<Zeroizing<String>>,
    needs_login: bool,
    next_due: u64,
    last_full: Option<u64>,
    remember_pending: bool,
    parallel_downloads: usize,
}
impl Service {
    /// Optional pacing for diagnostics; the desktop bounds concurrency instead.
    pub fn set_request_interval(&mut self, interval: Duration) {
        self.remote.set_interval(interval);
    }
    pub fn set_parallel_downloads(&mut self, count: usize) {
        self.parallel_downloads = count.clamp(1, 4);
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.clone()
    }
    pub fn execute(
        &mut self,
        command: Command,
        now: u64,
        cancel: &Cancellation,
        progress: &mut dyn FnMut(Snapshot),
    ) -> Result<()> {
        let result = self.handle(command, now, cancel, progress);
        self.state.syncing = false;
        if let Err(error) = &result {
            if cancel.is_canceled() {
                self.state.status = "Sync interrupted; downloaded games are kept".into();
                self.state.error = None;
            } else {
                self.state.error = Some(error.to_string());
                self.state.status = "Sync could not finish".into();
                if error.to_string().contains("sign-in required") {
                    self.needs_login = true;
                    self.password = None;
                }
            }
        }
        progress(self.snapshot());
        result
    }
    fn handle(
        &mut self,
        command: Command,
        now: u64,
        cancel: &Cancellation,
        progress: &mut dyn FnMut(Snapshot),
    ) -> Result<()> {
        match command {
            Command::SetLimit(limit) => {
                if limit.is_some_and(|n| n == 0 || n > 100000) {
                    return Err(
                        "Choose a history limit between 1 and 100,000 games, or all games".into(),
                    );
                }
                self.state.limit = limit;
                self.last_full = None;
                self.db.execute(
                    "UPDATE accounts SET last_full=NULL WHERE username=?1",
                    [&self.state.username],
                )?;
                self.save_config()?;
                if !self.state.automatic || self.state.username.is_empty() {
                    return Ok(());
                }
            }
            Command::Connect {
                username,
                password,
                remember,
            } => {
                let password = Zeroizing::new(password);
                let username = username.trim();
                let too_long = match self.state.server {
                    Server::Ogs => username.chars().count() > 128,
                    Server::Pandanet | Server::Fox => username.len() > 64,
                };
                if username.is_empty()
                    || too_long
                    || username.chars().any(|c| c.is_control())
                    || (self.state.server == Server::Pandanet && password.is_empty())
                {
                    return Err(format!(
                        "Enter your {} username{}",
                        self.state.server.label(),
                        if self.state.server == Server::Pandanet {
                            " and password"
                        } else {
                            ""
                        }
                    )
                    .into());
                }
                self.state.username = username.into();
                self.state.remember = remember && self.state.server == Server::Pandanet;
                self.state.automatic = true;
                self.password = Some(password);
                self.needs_login = false;
                self.remember_pending = true;
                self.load_account()?;
            }
            Command::SetAutomatic(enabled) => {
                self.state.automatic = enabled;
                self.save_config()?;
                self.state.error = None;
                self.state.status = if enabled {
                    "Automatic sync enabled"
                } else {
                    "Automatic sync paused"
                }
                .into();
                if !enabled {
                    return Ok(());
                }
                self.next_due = 0;
            }
            Command::Disconnect => {
                if self.state.server == Server::Pandanet {
                    self.credentials.forget(&self.state.username)?;
                }
                self.password = None;
                self.needs_login = true;
                self.state.remember = false;
                self.state.automatic = false;
                self.save_config()?;
                self.state.status = "Disconnected; downloaded games are kept".into();
                self.state.error = None;
                return Ok(());
            }
            Command::Tick => {
                if !self.state.automatic
                    || self.state.username.is_empty()
                    || self.needs_login
                    || now < self.next_due
                {
                    return Ok(());
                }
            }
            Command::SyncNow => {}
        }
        self.sync(now, cancel, progress)
    }
}
mod credentials;
pub use credentials::Keychain;
mod client;
pub use client::{Client, Clock, Options, SystemClock};
#[cfg(feature = "desktop")]
pub(crate) mod view;
