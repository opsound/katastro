//! Pandanet archive synchronization, independent of the desktop shell.
use crate::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use zeroize::Zeroizing;
mod http;
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
    pub limit: Option<usize>,
    pub failures: Vec<FailedGame>,
    pub username: String,
    pub remember: bool,
    pub automatic: bool,
    pub games: Vec<Game>,
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
    remote: http::Remote,
    credentials: Arc<dyn Credentials>,
    password: Option<Zeroizing<String>>,
    needs_login: bool,
    next_due: u64,
    last_full: Option<u64>,
    remember_pending: bool,
    parallel_downloads: usize,
}
impl Service {
    pub fn open(root: &Path, endpoint: &str, credentials: Arc<dyn Credentials>) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        }
        let db = crate::review::connection(&root.join("sync.sqlite"))?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS config (singleton INTEGER PRIMARY KEY CHECK(singleton=1),username TEXT NOT NULL,remember INTEGER NOT NULL,automatic INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS accounts (username TEXT PRIMARY KEY,last_sync INTEGER,last_full INTEGER);
            CREATE TABLE IF NOT EXISTS games (username TEXT NOT NULL,id TEXT NOT NULL,date TEXT NOT NULL,white TEXT NOT NULL,black TEXT NOT NULL,result TEXT NOT NULL,filename TEXT NOT NULL,sha TEXT NOT NULL,PRIMARY KEY(username,id));
            CREATE TABLE IF NOT EXISTS failures (username TEXT NOT NULL,id TEXT NOT NULL,date TEXT NOT NULL,message TEXT NOT NULL,PRIMARY KEY(username,id));")?;
        let has_limit = db
            .prepare("PRAGMA table_info(config)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == "max_games");
        if !has_limit {
            db.execute(
                "ALTER TABLE config ADD COLUMN max_games INTEGER DEFAULT 200",
                [],
            )?;
        }
        let saved = db
            .query_row(
                "SELECT username,remember,automatic,max_games FROM config WHERE singleton=1",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, bool>(1)?,
                        r.get::<_, bool>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;
        let mut state = Snapshot {
            status: "Connect your Pandanet account to sync games".into(),
            limit: Some(200),
            ..Snapshot::default()
        };
        if let Some((username, remember, automatic, limit)) = saved {
            state.limit = limit.map(|n| n.max(1) as usize);
            state.username = username;
            state.remember = remember;
            state.automatic = automatic;
            state.status = if automatic {
                "Waiting to sync"
            } else {
                "Automatic sync paused"
            }
            .into();
        }
        let mut service = Self {
            state,
            root: root.into(),
            db,
            remote: http::Remote::new(endpoint)?,
            credentials,
            password: None,
            needs_login: false,
            next_due: 0,
            last_full: None,
            remember_pending: false,
            parallel_downloads: 2,
        };
        service.load_account()?;
        Ok(service)
    }
    /// Optional pacing for diagnostics; the desktop bounds concurrency instead.
    pub fn set_request_interval(&mut self, interval: Duration) {
        self.remote.interval = interval;
    }
    pub fn set_parallel_downloads(&mut self, count: usize) {
        self.parallel_downloads = count.clamp(1, 4);
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.clone()
    }
    fn load_account(&mut self) -> Result<()> {
        let times = self
            .db
            .query_row(
                "SELECT last_sync,last_full FROM accounts WHERE username=?1",
                [&self.state.username],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
            .unwrap_or((None, None));
        self.state.failures = self
            .db
            .prepare("SELECT id,date,message FROM failures WHERE username=?1 ORDER BY date DESC")?
            .query_map([&self.state.username], |r| {
                Ok(FailedGame {
                    id: r.get(0)?,
                    date: r.get(1)?,
                    message: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        self.state.last_sync = times.0.map(|t| t as u64);
        self.last_full = times.1.map(|t| t as u64);
        let mut stmt=self.db.prepare("SELECT id,date,white,black,result,filename FROM games WHERE username=?1 ORDER BY date DESC,id")?;
        self.state.games = stmt
            .query_map([&self.state.username], |r| {
                Ok(Game {
                    id: r.get(0)?,
                    date: r.get(1)?,
                    white: r.get(2)?,
                    black: r.get(3)?,
                    result: r.get(4)?,
                    path: self.root.join(r.get::<_, String>(5)?),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(())
    }
    fn save_config(&self) -> Result<()> {
        self.db.execute("INSERT INTO config (singleton,username,remember,automatic,max_games) VALUES (1,?1,?2,?3,?4) ON CONFLICT(singleton) DO UPDATE SET username=excluded.username,remember=excluded.remember,automatic=excluded.automatic,max_games=excluded.max_games",params![self.state.username,self.state.remember,self.state.automatic,self.state.limit.map(|n|n as i64)])?;
        Ok(())
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
                if username.is_empty()
                    || username.len() > 64
                    || username.chars().any(|c| c.is_control())
                    || password.is_empty()
                {
                    return Err("Enter your Pandanet username and password".into());
                }
                self.state.username = username.into();
                self.state.remember = remember;
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
                self.credentials.forget(&self.state.username)?;
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
        self.next_due = now.saturating_add(SYNC_INTERVAL);
        self.state.error = None;
        if self.password.is_none() && self.state.remember {
            self.password = self
                .credentials
                .load(&self.state.username)?
                .map(Zeroizing::new);
        }
        if self.password.is_none() {
            self.needs_login = true;
            return Err("Pandanet sign-in required to sync games".into());
        }
        self.state.syncing = true;
        self.state.downloaded = 0;
        self.state.page = 0;
        self.state.pages = 0;
        self.state.status = "Connecting to Pandanet".into();
        progress(self.snapshot());
        self.remote.reset();
        let full = !self.state.failures.is_empty()
            || self
                .last_full
                .is_none_or(|last| now.saturating_sub(last) >= 24 * 60 * 60);
        let mut page_number = 1;
        let mut scanned = 0usize;
        let mut failed = 0usize;
        let mut seen = std::collections::HashSet::new();
        loop {
            cancel.check()?;
            let page = self.remote.page(
                &self.state.username,
                self.password.as_deref().unwrap(),
                page_number,
                cancel,
            )?;
            cancel.check()?;
            self.needs_login = false;
            if self.remember_pending {
                if self.state.remember {
                    self.credentials
                        .save(&self.state.username, self.password.as_deref().unwrap())?;
                } else {
                    self.credentials.forget(&self.state.username)?;
                }
                self.save_config()?;
                self.remember_pending = false;
            }
            self.state.page = page_number;
            self.state.pages = page.total;
            self.state.status = format!("Syncing page {page_number} of {}", page.total);
            progress(self.snapshot());
            let remaining = self
                .state
                .limit
                .map_or(usize::MAX, |limit| limit.saturating_sub(scanned));
            let entries: Vec<_> = page.entries.into_iter().take(remaining).collect();
            scanned += entries.len();
            let pending: Vec<_> = entries
                .into_iter()
                .filter(|entry| {
                    !self
                        .state
                        .games
                        .iter()
                        .any(|game| game.id == entry.game.id && game.path.is_file())
                })
                .filter(|entry| seen.insert(entry.game.id.clone()))
                .collect();
            let all_known = pending.is_empty();
            for batch in pending.chunks(self.parallel_downloads) {
                cancel.check()?;
                let results = if self.parallel_downloads == 1 {
                    batch
                        .iter()
                        .map(|entry| self.remote.download(entry.url.clone(), cancel))
                        .collect::<Vec<_>>()
                } else {
                    std::thread::scope(|scope| {
                        let handles: Vec<_> = batch
                            .iter()
                            .map(|entry| {
                                let mut remote = self.remote.clone();
                                scope.spawn(move || remote.download(entry.url.clone(), cancel))
                            })
                            .collect();
                        handles
                            .into_iter()
                            .map(|h| {
                                h.join().unwrap_or_else(|_| {
                                    Err("Pandanet download worker stopped".into())
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                };
                let mut backoff = None;
                for (entry, result) in batch.iter().zip(results) {
                    cancel.check()?;
                    let result =
                        result.and_then(|bytes| self.save_download(entry.game.clone(), &bytes));
                    match result {
                        Ok(()) => self.state.downloaded += 1,
                        Err(error) => {
                            let message = error.to_string();
                            if message.contains("HTTP 429") || message.contains("HTTP 503") {
                                backoff = Some(message.clone());
                            }
                            self.db.execute("INSERT INTO failures VALUES (?1,?2,?3,?4) ON CONFLICT(username,id) DO UPDATE SET message=excluded.message",params![self.state.username,entry.game.id,entry.game.date,message])?;
                            self.load_account()?;
                            failed += 1;
                        }
                    }
                    progress(self.snapshot());
                }
                if let Some(message) = backoff {
                    return Err(message.into());
                }
            }
            cancel.check()?;
            if page_number >= page.total
                || self.state.limit.is_some_and(|limit| scanned >= limit)
                || (!full && all_known)
            {
                break;
            }
            page_number += 1;
        }
        if failed > 0 {
            return Err(format!("{failed} games could not be downloaded; other games are saved. Failed games will be retried on the next sync.").into());
        }
        if full {
            self.last_full = Some(now);
        }
        self.db.execute("INSERT INTO accounts VALUES (?1,?2,?3) ON CONFLICT(username) DO UPDATE SET last_sync=excluded.last_sync,last_full=excluded.last_full",params![self.state.username,now as i64,self.last_full.map(|t|t as i64)])?;
        self.state.last_sync = Some(now);
        self.state.status = format!(
            "Up to date · {} games · {} downloaded",
            self.state.games.len(),
            self.state.downloaded
        );
        Ok(())
    }
    fn save_download(&mut self, game: Game, bytes: &[u8]) -> Result<()> {
        if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'<') {
            return Err("Pandanet returned an HTML page instead of an SGF; will retry".into());
        }
        crate::Document::parse(bytes).map_err(|e| format!("Could not import this SGF: {e}"))?;
        self.store(game, bytes)
    }
    fn store(&mut self, game: Game, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        let folder = PathBuf::from("games").join(crate::digest(self.state.username.as_bytes()));
        std::fs::create_dir_all(self.root.join(&folder))?;
        let filename = folder.join(format!("{}.sgf", game.id));
        let target = self.root.join(&filename);
        let hash = crate::digest(bytes);
        if target.exists() {
            if crate::digest(&std::fs::read(&target)?) != hash {
                return Err(
                    "A downloaded original has changed locally; it was not overwritten".into(),
                );
            }
        } else {
            let partial = target.with_extension("part");
            // Recover only our own unfinished transfer, never an SGF original.
            if partial.exists() {
                std::fs::remove_file(&partial)?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&partial)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            std::fs::hard_link(&partial, &target)?;
            std::fs::remove_file(&partial)?;
        }
        self.db.execute("INSERT INTO games VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(username,id) DO NOTHING",params![self.state.username,game.id,game.date,game.white,game.black,game.result,filename.to_string_lossy(),hash])?;
        self.db.execute(
            "DELETE FROM failures WHERE username=?1 AND id=?2",
            params![self.state.username, game.id],
        )?;
        self.load_account()?;
        Ok(())
    }
}
mod credentials;
pub use credentials::Keychain;
mod client;
pub use client::{Client, Clock, Options, SystemClock};
#[cfg(feature = "desktop")]
pub(crate) mod view;
