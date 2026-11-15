#[path = "support/pandanet/mod.rs"]
mod support;
use katastro::pandanet::{Client, Clock, Command, Options, Snapshot};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
struct Time(AtomicU64);
impl Clock for Time {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
fn until(c: &Client, f: impl Fn(&Snapshot) -> bool) -> Snapshot {
    loop {
        let s = c
            .snapshots
            .recv_timeout(Duration::from_secs(3))
            .expect("background sync must publish its result");
        if f(&s) {
            return s;
        }
    }
}
#[test]
fn background_sync_wakes_the_ui_restores_login_and_downloads_on_a_clock_tick() {
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, support::sgf(1))
        } else {
            (200, support::page(1, 1, &[1]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let time = Arc::new(Time(AtomicU64::new(100)));
    let options = || Options {
        server: Default::default(),
        root: tmp.path().into(),
        endpoint: fixture.endpoint.clone(),
        request_interval: Duration::ZERO,
        credentials: vault.clone(),
        clock: time.clone(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let c = Client::spawn(
        options(),
        Some(Arc::new(move || {
            let _ = tx.send(());
        })),
    );
    until(&c, |_| true);
    while rx.try_recv().is_ok() {}
    c.send(Command::Connect {
        username: "example".into(),
        password: "test-secret".into(),
        remember: true,
    })
    .unwrap();
    let s = until(&c, |s| s.last_sync == Some(100) && !s.syncing);
    assert_eq!(s.games.len(), 1);
    assert!(s.games[0].path.is_file());
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    time.0.store(1000, Ordering::SeqCst);
    c.send(Command::Tick).unwrap();
    until(&c, |s| s.last_sync == Some(1000) && !s.syncing);
    assert_eq!(fixture.downloads(), 1);
    drop(c);
    let c = Client::spawn(options(), None);
    let s = until(&c, |s| {
        s.last_sync == Some(1000) && !s.syncing && s.status.starts_with("Up to date")
    });
    assert_eq!(s.games.len(), 1);
    assert_eq!(fixture.downloads(), 1);
}

#[test]
fn closing_during_startup_does_not_start_a_new_sync_after_the_window_is_gone() {
    use katastro::pandanet::Cancellation;
    use std::sync::{Mutex, mpsc};
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, support::sgf(1))
        } else {
            (200, support::page(1, 1, &[1]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let mut s = fixture.service(tmp.path(), vault.clone());
    s.execute(
        Command::Connect {
            username: "example".into(),
            password: "test-secret".into(),
            remember: true,
        },
        100,
        &Cancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    drop(s);
    let before = fixture.requests.lock().unwrap().len();
    let (ready, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    let (finished, done) = mpsc::channel();
    struct Lifecycle(mpsc::Sender<()>);
    impl Clock for Lifecycle {
        fn now(&self) -> u64 {
            101
        }
    }
    impl Drop for Lifecycle {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let first = std::sync::atomic::AtomicBool::new(true);
    let c = Client::spawn(
        Options {
            server: Default::default(),
            root: tmp.path().into(),
            endpoint: fixture.endpoint.clone(),
            request_interval: Duration::ZERO,
            credentials: vault,
            clock: Arc::new(Lifecycle(finished)),
        },
        Some(Arc::new(move || {
            if first.swap(false, Ordering::SeqCst) {
                ready.send(()).unwrap();
                blocked.lock().unwrap().recv().unwrap();
            }
        })),
    );
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(c);
    release.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(3))
        .expect("worker must shut down after the app closes");
    assert_eq!(
        fixture.requests.lock().unwrap().len(),
        before,
        "closed UI must not launch a fresh automatic sync"
    );
}
