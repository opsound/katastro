#[path = "support/pandanet/mod.rs"]
mod support;
use katastro::{
    Point, Review,
    pandanet::{Cancellation, Command},
};
use std::sync::Arc;
use support::{Fixture, Vault, page, sgf};
fn connect() -> Command {
    Command::Connect {
        username: "example".into(),
        password: "test-secret".into(),
        remember: true,
    }
}

// Benefit: the entire archive becomes reviewable locally; a one-page-only importer
// or a restart that downloads everything again would remove that benefit.
#[test]
fn sync_paginates_persists_and_reopens_without_redownloading_or_losing_variations() {
    let fixture = Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            return (200, sgf(if r.url.contains("game2") { 2 } else { 1 }));
        }
        if r.body.contains("PageNo=2") {
            (200, page(2, 2, &[1]))
        } else {
            (200, page(1, 2, &[2]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::default());
    let mut service = fixture.service(tmp.path(), vault.clone());
    service
        .execute(connect(), 100, &Cancellation::default(), &mut |_| {})
        .unwrap();
    let state = service.snapshot();
    assert_eq!(state.games.len(), 2, "all archive pages must be downloaded");
    assert_eq!(fixture.downloads(), 2);
    assert!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.body.contains("userid=example")
                && r.body.contains("password=test-secret")
                && r.body.contains("pg=SearchResult")
                && r.body.contains("MyName=example"))
    );
    assert_eq!(
        vault.0.lock().unwrap().get("example").map(String::as_str),
        Some("test-secret")
    );
    let source = state.games[0].path.clone();
    let original = std::fs::read(&source).unwrap();
    let mut review = Review::new(
        &tmp.path().join("reviews.sqlite"),
        &tmp.path().join("cache.sqlite"),
    )
    .unwrap();
    review.import(&source).unwrap();
    review.select(1).unwrap();
    review.play(Some(Point::new(3, 3))).unwrap();
    drop(service);
    let before_reopen = fixture.requests.lock().unwrap().len();
    let mut reopened = fixture.service(tmp.path(), vault);
    reopened
        .execute(Command::Tick, 101, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        fixture.downloads(),
        2,
        "remembered login reuses persisted downloads"
    );
    assert_eq!(reopened.snapshot().games.len(), 2);
    assert!(
        fixture.requests.lock().unwrap().len() > before_reopen,
        "reopening must check the archive using remembered login"
    );
    assert_eq!(std::fs::read(source.clone()).unwrap(), original);
    review.import(&source).unwrap();
    assert_eq!(review.document().unwrap().nodes.len(), 5);
    assert_eq!(review.document().unwrap().mainline.len(), 4);
    for path in [
        tmp.path().join("sync.sqlite"),
        tmp.path().join("sync.sqlite-wal"),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            assert!(!bytes.windows(11).any(|x| x == b"test-secret"));
            assert!(!bytes.windows(15).any(|x| x == b"private-session"));
        }
    }
}

// Benefit: a new archive session reuses originals, while deleting one original
// does not leave that game permanently unavailable for review.
#[test]
fn changed_session_links_reuse_originals_and_missing_files_are_downloaded_again() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let session = Arc::new(AtomicUsize::new(1));
    let flag = session.clone();
    let fixture = Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            (200, sgf(if r.url.contains("game2") { 2 } else { 1 }))
        } else {
            (
                200,
                page(1, 1, &[2, 1]).replace(
                    "private-session",
                    &format!("session-{}", flag.load(Ordering::SeqCst)),
                ),
            )
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::default());
    let mut s = fixture.service(tmp.path(), vault.clone());
    s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
        .unwrap();
    let path = s.snapshot().games[0].path.clone();
    let original = std::fs::read(&path).unwrap();
    drop(s);
    session.store(2, Ordering::SeqCst);
    let mut s = fixture.service(tmp.path(), vault);
    s.execute(Command::Tick, 101, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        fixture.downloads(),
        2,
        "session changes must not change game identity"
    );
    assert_eq!(s.snapshot().games.len(), 2);
    std::fs::remove_file(&path).unwrap();
    s.execute(Command::SyncNow, 102, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        fixture.downloads(),
        3,
        "only the missing original needs another download"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let mut review = Review::new(
        &tmp.path().join("reviews.sqlite"),
        &tmp.path().join("cache.sqlite"),
    )
    .unwrap();
    review.import(&path).unwrap();
    review.select(3).unwrap();
    assert_eq!(review.document().unwrap().mainline.len(), 4);
}

// Benefit: new games arrive on schedule without interaction; merely syncing on
// button clicks or checking the wrong deadline would leave the library stale.
#[test]
fn automatic_sync_uses_the_supplied_clock_and_pause_survives_reopen() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let newer = Arc::new(AtomicBool::new(false));
    let flag = newer.clone();
    let fixture = Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            (200, sgf(if r.url.contains("game2") { 2 } else { 1 }))
        } else {
            (
                200,
                page(
                    1,
                    1,
                    if flag.load(Ordering::SeqCst) {
                        &[2, 1]
                    } else {
                        &[1]
                    },
                ),
            )
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::default());
    let mut s = fixture.service(tmp.path(), vault.clone());
    s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
        .unwrap();
    let before = fixture.requests.lock().unwrap().len();
    newer.store(true, Ordering::SeqCst);
    s.execute(Command::Tick, 999, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(fixture.requests.lock().unwrap().len(), before);
    s.execute(Command::Tick, 1000, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        s.snapshot().games.len(),
        2,
        "scheduled sync must import the new game"
    );
    assert_eq!(fixture.downloads(), 2);
    s.execute(
        Command::SetAutomatic(false),
        1001,
        &Cancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    let before = fixture.requests.lock().unwrap().len();
    drop(s);
    let mut s = fixture.service(tmp.path(), vault);
    s.execute(Command::Tick, 2000, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        fixture.requests.lock().unwrap().len(),
        before,
        "paused sync must stay paused across restart"
    );
}

// Benefit: a canceled/failed initial import resumes without losing the backfill.
#[test]
fn cancellation_and_retry_preserve_downloads_and_finish_older_pages() {
    let fixture = Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, sgf(if r.url.contains("game2") { 2 } else { 1 }))
        } else if r.body.contains("PageNo=2") {
            (200, page(2, 2, &[1]))
        } else {
            (200, page(1, 2, &[2]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::default());
    let mut s = fixture.service(tmp.path(), vault);
    let cancel = Cancellation::default();
    let _ = s.execute(connect(), 100, &cancel, &mut |state| {
        if state.downloaded == 1 {
            cancel.cancel()
        }
    });
    assert_eq!(
        s.snapshot().games.len(),
        1,
        "finished downloads survive cancellation"
    );
    assert!(s.snapshot().last_sync.is_none());
    s.execute(Command::SyncNow, 101, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        s.snapshot().games.len(),
        2,
        "resuming must not stop at the already-known first page"
    );
    assert_eq!(fixture.downloads(), 2);
}

#[test]
fn transient_failure_retries_on_schedule_without_refetching_finished_games() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let fail = Arc::new(AtomicBool::new(true));
    let flag = fail.clone();
    let fixture = Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            if r.url.contains("game1") && flag.load(Ordering::SeqCst) {
                (503, "busy".into())
            } else {
                (200, sgf(if r.url.contains("game2") { 2 } else { 1 }))
            }
        } else {
            (200, page(1, 1, &[2, 1]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut s = fixture.service(tmp.path(), Arc::new(Vault::default()));
    assert!(
        s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
            .is_err()
    );
    assert_eq!(s.snapshot().games.len(), 1);
    assert!(s.snapshot().last_sync.is_none());
    fail.store(false, Ordering::SeqCst);
    s.execute(Command::Tick, 1000, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(s.snapshot().games.len(), 2);
    assert_eq!(fixture.downloads(), 3);
}

// Benefit: login/setup/error pages and corrupt downloads never masquerade as
// successful synchronization, erase the library, or save an invalid password.
#[test]
fn login_and_malformed_responses_are_errors_not_empty_archives() {
    for body in [
        "<form><input type=password></form>",
        "<html>Maintenance</html>",
    ] {
        let fixture = Fixture::new(move |_| (200, body.into()));
        let tmp = tempfile::tempdir().unwrap();
        let vault = Arc::new(Vault::default());
        let mut s = fixture.service(tmp.path(), vault.clone());
        assert!(
            s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
                .is_err(),
            "HTML error page must fail"
        );
        assert!(s.snapshot().last_sync.is_none());
        assert!(vault.0.lock().unwrap().is_empty());
    }
    let fixture = Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, "<html>Not SGF</html>".into())
        } else {
            (200, page(1, 1, &[1]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut s = fixture.service(tmp.path(), Arc::new(Vault::default()));
    assert!(
        s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
            .is_err()
    );
    assert!(s.snapshot().games.is_empty());
    assert!(s.snapshot().last_sync.is_none());
}

#[test]
fn session_only_login_is_forgotten_and_disconnect_removes_remembered_credentials() {
    let fixture = Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, sgf(1))
        } else {
            (200, page(1, 1, &[1]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::default());
    let mut s = fixture.service(tmp.path(), vault.clone());
    let cmd = Command::Connect {
        username: "example".into(),
        password: "test-secret".into(),
        remember: false,
    };
    s.execute(cmd, 100, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(s.snapshot().games.len(), 1);
    assert!(vault.0.lock().unwrap().is_empty());
    drop(s);
    let before = fixture.requests.lock().unwrap().len();
    let mut s = fixture.service(tmp.path(), vault.clone());
    let _ = s.execute(Command::Tick, 101, &Cancellation::default(), &mut |_| {});
    assert_eq!(fixture.requests.lock().unwrap().len(), before);
    s.execute(connect(), 102, &Cancellation::default(), &mut |_| {})
        .unwrap();
    s.execute(
        Command::Disconnect,
        103,
        &Cancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    assert!(vault.0.lock().unwrap().is_empty());
    assert_eq!(
        s.snapshot().games.len(),
        1,
        "disconnect retains downloaded originals"
    );
    let before = fixture.requests.lock().unwrap().len();
    s.execute(Command::Tick, 2000, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(fixture.requests.lock().unwrap().len(), before);
}

#[test]
fn meta_refresh_loads_the_archive_and_foreign_account_or_foreign_host_is_rejected() {
    let fixture = Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, sgf(1))
        } else if r.body.is_empty() {
            (200, page(1, 1, &[1]))
        } else {
            (
                200,
                r#"<meta http-equiv="refresh" content="0; URL=/archive?key=private-session">"#
                    .into(),
            )
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut s = fixture.service(tmp.path(), Arc::new(Vault::default()));
    s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(s.snapshot().games.len(), 1);
    for body in [
        page(1, 1, &[1]).replace("example", "someoneelse"),
        r#"<meta http-equiv="refresh" content="0; URL=https://example.org/?key=private-session">"#
            .into(),
    ] {
        let fixture = Fixture::new(move |_| (200, body.clone()));
        let tmp = tempfile::tempdir().unwrap();
        let mut s = fixture.service(tmp.path(), Arc::new(Vault::default()));
        assert!(
            s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
                .is_err()
        );
        assert!(s.snapshot().games.is_empty());
        assert!(
            !s.snapshot()
                .error
                .unwrap_or_default()
                .contains("private-session")
        );
    }
}

// Closing during the first import must resume automatically on the next launch;
// stopping an in-flight worker must not silently disable the saved preference.
#[test]
fn interrupted_import_resumes_automatically_after_restart() {
    let fixture = Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, sgf(if r.url.contains("game2") { 2 } else { 1 }))
        } else {
            (200, page(1, 1, &[2, 1]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::default());
    let mut s = fixture.service(tmp.path(), vault.clone());
    let cancel = Cancellation::default();
    let _ = s.execute(connect(), 100, &cancel, &mut |state| {
        if state.downloaded == 1 {
            cancel.cancel()
        }
    });
    assert_eq!(s.snapshot().games.len(), 1);
    drop(s);
    let mut s = fixture.service(tmp.path(), vault);
    s.execute(Command::Tick, 101, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        s.snapshot().games.len(),
        2,
        "relaunch must resume an interrupted automatic import"
    );
    assert_eq!(
        fixture.downloads(),
        3,
        "a canceled in-flight result is retried; completed files are reused"
    );
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.url.contains("mode=sgf") && r.url.contains("game2"))
            .count(),
        1
    );
}

#[test]
fn daily_sweep_finds_late_archived_games_behind_an_already_known_page() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let older = Arc::new(AtomicBool::new(false));
    let flag = older.clone();
    let fixture = Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            (200, sgf(if r.url.contains("game2") { 2 } else { 1 }))
        } else if r.body.contains("PageNo=2") {
            (200, page(2, 2, &[1]))
        } else {
            (
                200,
                page(1, if flag.load(Ordering::SeqCst) { 2 } else { 1 }, &[2]),
            )
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut s = fixture.service(tmp.path(), Arc::new(Vault::default()));
    s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
        .unwrap();
    older.store(true, Ordering::SeqCst);
    s.execute(Command::Tick, 1000, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(s.snapshot().games.len(), 1);
    s.execute(Command::Tick, 86500, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        s.snapshot().games.len(),
        2,
        "daily sweep must go beyond a fully known first page"
    );
    assert_eq!(fixture.downloads(), 2);
}

#[test]
fn successful_manual_signin_retry_reenables_scheduled_checks() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let phase = Arc::new(AtomicUsize::new(0));
    let flag = phase.clone();
    let fixture = Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            (200, sgf(if r.url.contains("game2") { 2 } else { 1 }))
        } else if flag.load(Ordering::SeqCst) == 1 {
            (200, "<form><input type=password></form>".into())
        } else {
            (
                200,
                page(
                    1,
                    1,
                    if flag.load(Ordering::SeqCst) == 3 {
                        &[2, 1]
                    } else {
                        &[1]
                    },
                ),
            )
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut s = fixture.service(tmp.path(), Arc::new(Vault::default()));
    s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
        .unwrap();
    phase.store(1, Ordering::SeqCst);
    assert!(
        s.execute(Command::Tick, 1000, &Cancellation::default(), &mut |_| {})
            .is_err()
    );
    phase.store(2, Ordering::SeqCst);
    s.execute(
        Command::SyncNow,
        1001,
        &Cancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    phase.store(3, Ordering::SeqCst);
    s.execute(Command::Tick, 1901, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        s.snapshot().games.len(),
        2,
        "a recovered login must restore automatic checks"
    );
}

#[test]
fn history_limit_is_persisted_and_expansion_backfills_without_deleting_older_downloads() {
    let fixture = Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (
                200,
                sgf(if r.url.contains("game3") {
                    3
                } else if r.url.contains("game2") {
                    2
                } else {
                    1
                }),
            )
        } else {
            (200, page(1, 1, &[3, 2, 1]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::default());
    let mut s = fixture.service(tmp.path(), vault.clone());
    s.execute(
        Command::SetLimit(Some(2)),
        100,
        &Cancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    s.execute(connect(), 101, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        s.snapshot().games.len(),
        2,
        "only the newest selected number of games should download"
    );
    assert_eq!(fixture.downloads(), 2);
    drop(s);
    let mut s = fixture.service(tmp.path(), vault);
    s.execute(Command::Tick, 102, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(fixture.downloads(), 2);
    assert_eq!(s.snapshot().limit, Some(2));
    s.execute(
        Command::SetLimit(Some(3)),
        103,
        &Cancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(s.snapshot().games.len(), 3);
    assert_eq!(fixture.downloads(), 3);
    s.execute(
        Command::SetLimit(Some(1)),
        104,
        &Cancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        s.snapshot().games.len(),
        3,
        "lowering a limit must retain existing originals"
    );
    assert_eq!(fixture.downloads(), 3);
}

#[test]
fn a_bad_download_does_not_block_other_games_and_is_retried_on_the_next_check() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let bad = Arc::new(AtomicBool::new(true));
    let flag = bad.clone();
    let fixture = Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            if r.url.contains("game2") && flag.load(Ordering::SeqCst) {
                (200, "<html>Temporary server error</html>".into())
            } else {
                (200, sgf(if r.url.contains("game2") { 2 } else { 1 }))
            }
        } else {
            (200, page(1, 1, &[2, 1]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut s = fixture.service(tmp.path(), Arc::new(Vault::default()));
    assert!(
        s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
            .is_err()
    );
    assert_eq!(
        s.snapshot().games.len(),
        1,
        "a bad individual download must not block later games"
    );
    assert_eq!(s.snapshot().failures.len(), 1);
    assert!(s.snapshot().failures[0].message.contains("HTML"));
    bad.store(false, Ordering::SeqCst);
    s.execute(Command::Tick, 1000, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(s.snapshot().games.len(), 2);
    assert!(s.snapshot().failures.is_empty());
    assert_eq!(fixture.downloads(), 3);
}

#[test]
fn downloads_overlap_with_a_bounded_number_of_requests_in_flight() {
    use std::{
        sync::{
            Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let endpoint = format!("http://{}/cgi-bin/cgi.exe?MH", server.server_addr());
    let (started, arrived) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let listener = server.clone();
    let server_gate = gate.clone();
    let max = maximum.clone();
    let serving = thread::spawn(move || {
        let mut children = Vec::new();
        while let Ok(req) = listener.recv() {
            let started = started.clone();
            let gate = server_gate.clone();
            let active = active.clone();
            let max = max.clone();
            children.push(thread::spawn(move || {
                let body = if req.url().contains("mode=sgf") {
                    let n = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(n, Ordering::SeqCst);
                    let _ = started.send(());
                    let (lock, cv) = &*gate;
                    let mut open = lock.lock().unwrap();
                    while !*open {
                        open = cv.wait(open).unwrap();
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    sgf(if req.url().contains("game2") { 2 } else { 1 })
                } else {
                    page(1, 1, &[2, 1])
                };
                let _ = req.respond(tiny_http::Response::from_string(body));
            }));
        }
        for c in children {
            c.join().unwrap();
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let syncing = thread::spawn(move || {
        let mut s = katastro::pandanet::Service::open(&root, &endpoint, Arc::new(Vault::default()))
            .unwrap();
        s.set_request_interval(Duration::ZERO);
        s.set_parallel_downloads(2);
        s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
            .unwrap();
        s.snapshot()
    });
    let first = arrived.recv_timeout(Duration::from_secs(3));
    let second = arrived.recv_timeout(Duration::from_secs(3));
    {
        let (lock, cv) = &*gate;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    let state = syncing.join().unwrap();
    server.unblock();
    serving.join().unwrap();
    assert!(
        first.is_ok() && second.is_ok(),
        "two downloads must enter the server before either is released"
    );
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    assert_eq!(state.games.len(), 2);
}

#[test]
fn failed_games_are_retried_even_when_a_newer_page_is_fully_downloaded() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let phase = Arc::new(AtomicUsize::new(0));
    let flag = phase.clone();
    let fixture = Fixture::new(move |r| {
        let phase = flag.load(Ordering::SeqCst);
        if r.url.contains("mode=sgf") {
            let id = r
                .url
                .split("v1=game")
                .nth(1)
                .unwrap()
                .split('&')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            if id == 3 && phase == 1 {
                (200, "<html>temporary error</html>".into())
            } else {
                (200, sgf(id))
            }
        } else if phase == 0 {
            if r.body.contains("PageNo=2") {
                (200, page(2, 2, &[1]))
            } else {
                (200, page(1, 2, &[2]))
            }
        } else if r.body.contains("PageNo=3") {
            (200, page(3, 3, &[1]))
        } else if r.body.contains("PageNo=2") {
            (200, page(2, 3, &[3, 2]))
        } else {
            (200, page(1, 3, &[4]))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut s = fixture.service(tmp.path(), Arc::new(Vault::default()));
    s.execute(connect(), 100, &Cancellation::default(), &mut |_| {})
        .unwrap();
    phase.store(1, Ordering::SeqCst);
    assert!(
        s.execute(Command::Tick, 1000, &Cancellation::default(), &mut |_| {})
            .is_err()
    );
    assert_eq!(s.snapshot().games.len(), 3);
    phase.store(2, Ordering::SeqCst);
    s.execute(Command::Tick, 1900, &Cancellation::default(), &mut |_| {})
        .unwrap();
    assert_eq!(
        s.snapshot().games.len(),
        4,
        "known newer pages must not hide a failed older download"
    );
    assert!(s.snapshot().failures.is_empty());
}
