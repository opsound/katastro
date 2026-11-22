//! One background scanner for the visible Games window.
use crate::{
    EngineProfile, Review,
    archive::{
        Game,
        categories::{Category, classify},
    },
    engine::{Engine, EngineConfig},
};
use eframe::egui;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

type Rows = Vec<(String, Vec<Game>)>;
#[derive(Default)]
struct Mailbox {
    revision: u64,
    request: Option<(EngineConfig, Rows)>,
    focus: Option<PathBuf>,
    results: HashMap<PathBuf, Category>,
    clear_results: bool,
}

pub(super) struct Scanner {
    mail: Arc<Mutex<Mailbox>>,
    wake: mpsc::SyncSender<()>,
}

impl Scanner {
    pub(super) fn new(databases: (PathBuf, PathBuf), ctx: egui::Context) -> Self {
        let mail = Arc::new(Mutex::new(Mailbox::default()));
        let (wake, rx) = mpsc::sync_channel(1);
        let shared = mail.clone();
        std::thread::spawn(move || {
            let mut scan = None;
            let mut revision = 0;
            let mut review = None;
            let mut cached_profile = Profile::default();
            loop {
                let event = if scan.is_some() {
                    rx.recv_timeout(Duration::from_secs(1))
                } else {
                    rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
                };
                if matches!(event, Err(mpsc::RecvTimeoutError::Disconnected)) {
                    return;
                }
                let focus = {
                    let mut mail = shared.lock().unwrap();
                    if revision != mail.revision {
                        revision = mail.revision;
                        scan = mail
                            .request
                            .take()
                            .map(|(config, rows)| Scan::new(config, rows));
                    }
                    mail.focus.clone()
                };
                if let Some(scan) = &mut scan {
                    if review.is_none() {
                        review = Review::new(&databases.0, &databases.1).ok();
                    }
                    let profile = cached_profile.resolve(&scan.config, review.as_ref());
                    scan.refresh(
                        review.as_ref(),
                        profile,
                        focus.as_deref(),
                        |game, category, clear| {
                            let mut mail = shared.lock().unwrap();
                            if mail.revision != revision {
                                return false;
                            }
                            if clear {
                                mail.results.clear();
                                mail.clear_results = true;
                            }
                            let wake = mail.results.is_empty();
                            mail.results.insert(game.path.clone(), category);
                            drop(mail);
                            if wake {
                                ctx.request_repaint();
                            }
                            true
                        },
                    );
                }
            }
        });
        Self { mail, wake }
    }

    pub(super) fn request(&self, request: Option<(EngineConfig, Rows)>) {
        let mut mail = self.mail.lock().unwrap();
        mail.revision += 1;
        mail.request = request;
        mail.results.clear();
        let _ = self.wake.try_send(());
    }

    pub(super) fn collect(&self, view: &mut HashMap<PathBuf, Category>, focus: Option<&Path>) {
        let mut mail = self.mail.lock().unwrap();
        if mail.focus.as_deref() != focus {
            mail.focus = focus.map(Path::to_owned);
        }
        if std::mem::take(&mut mail.clear_results) {
            view.clear();
        }
        view.extend(mail.results.drain());
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.request(None);
    }
}

#[cfg(unix)]
type FileId = (u64, u64, i64, i64);
#[cfg(not(unix))]
type FileId = (std::time::SystemTime, u64);

#[cfg(unix)]
fn file_id(path: &Path) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    let metadata = path.metadata().ok()?;
    Some((
        metadata.dev(),
        metadata.ino(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    ))
}
#[cfg(not(unix))]
fn file_id(path: &Path) -> Option<FileId> {
    let metadata = path.metadata().ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

#[derive(Default)]
struct Profile {
    files: Option<[Option<FileId>; 2]>,
    current: Option<EngineProfile>,
}
impl Profile {
    fn resolve(&mut self, config: &EngineConfig, review: Option<&Review>) -> Option<EngineProfile> {
        let files = [file_id(&config.executable), file_id(&config.model)];
        if self.files.as_ref() != Some(&files) {
            self.current = Engine::profile(config).ok();
            self.files = Some(files);
        }
        self.current
            .clone()
            .or_else(|| review?.last_profile().ok().flatten())
    }
}

#[derive(Default)]
struct Scan {
    config: EngineConfig,
    rows: Rows,
    aliases: HashMap<PathBuf, PathBuf>,
    last_full: Option<(Option<i64>, Option<EngineProfile>, Instant)>,
    last_version: Option<i64>,
}
impl Scan {
    fn new(config: EngineConfig, rows: Rows) -> Self {
        Self {
            config,
            rows,
            ..Default::default()
        }
    }
    fn refresh(
        &mut self,
        review: Option<&Review>,
        profile: Option<EngineProfile>,
        focus: Option<&Path>,
        mut publish: impl FnMut(&Game, Category, bool) -> bool,
    ) {
        let version = review.and_then(|r| {
            r.cache
                .pragma_query_value(None, "data_version", |row| row.get(0))
                .ok()
        });
        // Large libraries scan immediately on open or model changes. While a game
        // is being refined, refresh that game first and coalesce the full sweeps.
        let full = self
            .last_full
            .as_ref()
            .is_none_or(|(previous, old_profile, last)| {
                old_profile != &profile
                    || (previous != &version
                        && (self
                            .rows
                            .iter()
                            .map(|(_, games)| games.len())
                            .sum::<usize>()
                            <= 16
                            || last.elapsed() >= Duration::from_secs(30)))
            });
        if !full && self.last_version == version {
            return;
        }
        self.last_version = version;
        let mut clear = self
            .last_full
            .as_ref()
            .is_some_and(|(_, old, _)| old != &profile);
        for (account, games) in &self.rows {
            for game in games {
                if self.last_full.is_none()
                    && let Ok(path) = game.path.canonicalize()
                    && path != game.path
                {
                    self.aliases.insert(path, game.path.clone());
                }
                if full
                    || focus
                        .is_some_and(|p| p == game.path || self.aliases.get(p) == Some(&game.path))
                {
                    let category = review
                        .and_then(|r| classify(r, game, account, profile.as_ref()).ok())
                        .unwrap_or(Category::Unavailable);
                    if !publish(game, category, std::mem::take(&mut clear)) {
                        return;
                    }
                }
            }
        }
        if full {
            self.last_full = Some((version, profile, Instant::now()));
        }
    }
}
