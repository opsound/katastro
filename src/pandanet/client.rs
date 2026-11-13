use super::{Cancellation, Command, Credentials, Keychain, Service, Snapshot};
use crate::Result;
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
    pub root: PathBuf,
    pub endpoint: String,
    pub request_interval: Duration,
    pub credentials: Arc<dyn Credentials>,
    pub clock: Arc<dyn Clock>,
}
impl Options {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            endpoint: super::ENDPOINT.into(),
            request_interval: Duration::ZERO,
            credentials: Arc::new(Keychain::default()),
            clock: Arc::new(SystemClock),
        }
    }
}
type Wake = Option<Arc<dyn Fn() + Send + Sync>>;
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
        let (tx, snapshots) = mpsc::channel();
        let controls = Arc::new(Controls::default());
        let worker_controls = controls.clone();
        std::thread::spawn(move || {
            let publish = |state| {
                let _ = tx.send(state);
                if let Some(wake) = &wake {
                    wake();
                }
            };
            let mut service =
                match Service::open(&options.root, &options.endpoint, options.credentials) {
                    Ok(service) => service,
                    Err(e) => {
                        publish(Snapshot {
                            error: Some(e.to_string()),
                            status: "Could not open Pandanet library".into(),
                            ..Snapshot::default()
                        });
                        return;
                    }
                };
            service.set_request_interval(options.request_interval);
            publish(service.snapshot());
            let first = worker_controls.token();
            if !first.is_canceled() {
                let _ = service.execute(Command::Tick, options.clock.now(), &first, &mut |s| {
                    publish(s)
                });
            }
            drop(first);
            loop {
                let now = options.clock.now();
                let seconds = if service.state.automatic && !service.needs_login {
                    service.next_due.saturating_sub(now).clamp(1, 60)
                } else {
                    60
                };
                let work = match rx.recv_timeout(Duration::from_secs(seconds)) {
                    Ok(work) => work,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        (Command::Tick, worker_controls.token())
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                if work.1.is_canceled() {
                    continue;
                }
                let _ = service.execute(work.0, options.clock.now(), &work.1, &mut |s| publish(s));
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
            .map_err(|_| "Pandanet sync worker stopped".into())
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.controls.closed.store(true, Ordering::SeqCst);
        self.controls.cancel();
    }
}
