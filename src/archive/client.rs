use super::{Cancellation, Command, Credentials, Keychain, Service, Snapshot};
use crate::Result;
use crate::updates::{self, Wake};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub trait Clock: Send + Sync {
    fn now(&self) -> u64;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}
pub struct Options {
    pub server: super::Server,
    pub root: PathBuf,
    pub endpoint: String,
    pub request_interval: Duration,
    pub credentials: Arc<dyn Credentials>,
    pub clock: Arc<dyn Clock>,
}
impl Options {
    pub fn ogs(root: PathBuf) -> Self {
        Self {
            server: super::Server::Ogs,
            endpoint: super::ogs::ENDPOINT.into(),
            ..Self::new(root)
        }
    }

    pub fn fox(root: PathBuf) -> Self {
        Self {
            server: super::Server::Fox,
            endpoint: super::fox::ENDPOINT.into(),
            ..Self::new(root)
        }
    }

    pub fn new(root: PathBuf) -> Self {
        Self {
            server: super::Server::Pandanet,
            root,
            endpoint: super::ENDPOINT.into(),
            request_interval: Duration::ZERO,
            credentials: Arc::new(Keychain::default()),
            clock: Arc::new(SystemClock),
        }
    }
}
type Work = (Command, Arc<Cancellation>);
#[derive(Default)]
struct Controls {
    tokens: Mutex<Vec<Weak<Cancellation>>>,
    closed: AtomicBool,
}
impl Controls {
    fn cancel(&self) {
        for token in self.tokens.lock().unwrap().iter().filter_map(Weak::upgrade) {
            token.cancel();
        }
    }
    fn token(&self) -> Arc<Cancellation> {
        let token = Arc::new(Cancellation::default());
        let mut tokens = self.tokens.lock().unwrap();
        tokens.retain(|t| t.strong_count() > 0);
        tokens.push(Arc::downgrade(&token));
        if self.closed.load(Ordering::SeqCst) {
            token.cancel();
        }
        token
    }
}
pub struct Client {
    pub snapshots: Receiver<Snapshot>,
    commands: Sender<Work>,
    controls: Arc<Controls>,
}
impl Client {
    pub fn spawn(options: Options, wake: Wake) -> Self {
        let (commands, rx) = mpsc::channel::<Work>();
        let (publish, snapshots) = updates::channel(wake);
        let controls = Arc::new(Controls::default());
        let worker_controls = controls.clone();
        std::thread::spawn(move || {
            let mut service = match Service::open_for(
                &options.root,
                &options.endpoint,
                options.credentials,
                options.server,
            ) {
                Ok(service) => service,
                Err(e) => {
                    publish(Snapshot {
                        error: Some(e.to_string()),
                        server: options.server,
                        status: format!("Could not open {} library", options.server.label()),
                        ..Snapshot::default()
                    });
                    return;
                }
            };
            service.set_request_interval(options.request_interval);
            publish(service.snapshot());
            let mut work = (Command::Tick, worker_controls.token());
            loop {
                let (command, cancel) = work;
                if !cancel.is_canceled() {
                    let _ =
                        service.execute(command, options.clock.now(), &cancel, &mut |s| publish(s));
                }
                drop(cancel);
                let now = options.clock.now();
                let seconds = if service.state.automatic && !service.needs_login {
                    service.next_due.saturating_sub(now).clamp(1, 60)
                } else {
                    60
                };
                work = match rx.recv_timeout(Duration::from_secs(seconds)) {
                    Ok(work) => work,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        (Command::Tick, worker_controls.token())
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
            }
        });
        Self {
            snapshots,
            commands,
            controls,
        }
    }
    pub fn send(&self, command: Command) -> Result<()> {
        self.controls.cancel();
        self.commands
            .send((command, self.controls.token()))
            .map_err(|_| "Archive sync worker stopped".into())
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.controls.closed.store(true, Ordering::SeqCst);
        self.controls.cancel();
    }
}
