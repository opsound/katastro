#[path = "support/pandanet/mod.rs"]
mod support;
use katastro::{
    Document,
    pandanet::{Cancellation, Command, SYNC_INTERVAL, Server, Service},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn connect() -> Command {
    Command::Connect {
        username: "example".into(),
        password: String::new(),
        remember: false,
    }
}
fn execute(service: &mut Service, command: Command, now: u64) {
    service
        .execute(command, now, &Cancellation::default(), &mut |_| {})
        .unwrap();
}
fn record(id: usize) -> serde_json::Value {
    json!({"chessid":id.to_string(),"blackuid":42,"whiteuid":55,"blacknick":"example","whitenick":"opponent","blackenname":"example","whiteenname":"opponent","starttime":format!("2026-11-{:02} 12:00:00",id%28+1)})
}
fn sgf(id: usize) -> String {
    format!(
        "(;GM[1]FF[4]SZ[9]AP[foxwq]KM[350]RU[Chinese]PB[example]BR[3k]PW[opponent]WR[2k]RE[B+R]DT[2026-11-01]GN[Game {id}];B[cc];W[gg])"
    )
}
fn query(url: &str, name: &str) -> String {
    reqwest::Url::parse(&format!("http://localhost{url}"))
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}
// Benefit: capped automatic Fox imports survive restart and skip saved originals.
// Regression: repeated first pages, forgotten preferences, or duplicate downloads.
#[test]
fn fox_sync_caps_pages_resumes_and_reuses_originals_without_credentials() {
    let downloads = Arc::new(AtomicUsize::new(0));
    let count = downloads.clone();
    let fixture = support::Fixture::new(move |r| {
        assert!(r.body.is_empty(), "Fox must not send a login/password");
        let response = if r.url.contains("QueryUserInfoPanel") {
            assert_eq!(query(&r.url, "username"), "example");
            json!({"result":0,"uid":"42","username":"example","englishname":"example","hide_game_record":0})
        } else if r.url.contains("FetchChessList") {
            assert_eq!(query(&r.url, "dstuid"), "42");
            let ids = match query(&r.url, "lastCode").as_str() {
                "0" => (101..=200).rev().collect::<Vec<_>>(),
                "101" => (1..=100).rev().collect(),
                other => panic!("bad cursor {other}"),
            };
            json!({"result":0,"dstuid":"42","chesslist":ids.into_iter().map(record).collect::<Vec<_>>()})
        } else if r.url.contains("FetchChess") {
            count.fetch_add(1, Ordering::SeqCst);
            let id = query(&r.url, "chessid").parse::<usize>().unwrap();
            json!({"result":0,"chessid":id.to_string(),"chess":sgf(id)})
        } else {
            panic!("unexpected request {}", r.url)
        };
        (200, response.to_string())
    });
    let root = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let mut service =
        Service::open_for(root.path(), &fixture.endpoint, vault.clone(), Server::Fox).unwrap();
    execute(&mut service, Command::SetLimit(Some(102)), 1);
    execute(&mut service, connect(), 2);
    let saved = service.snapshot();
    assert_eq!(saved.games.len(), 102);
    assert_eq!(saved.server, Server::Fox);
    assert_eq!(downloads.load(Ordering::SeqCst), 102);
    assert!(vault.0.lock().unwrap().is_empty());
    let first = saved.games.iter().find(|g| g.id == "200").unwrap();
    let bytes = std::fs::read(&first.path).unwrap();
    assert_eq!(bytes, sgf(200).as_bytes());
    assert_eq!(Document::parse(&bytes).unwrap().mainline.len(), 3);
    assert_eq!(first.black, "example 3k");
    assert_eq!(first.result, "B+R");
    drop(service);
    let mut service =
        Service::open_for(root.path(), &fixture.endpoint, vault, Server::Fox).unwrap();
    execute(&mut service, Command::Tick, 3);
    assert_eq!(service.snapshot().games.len(), 102);
    assert_eq!(downloads.load(Ordering::SeqCst), 102);
    std::fs::remove_file(&first.path).unwrap();
    execute(&mut service, Command::Tick, 3 + SYNC_INTERVAL);
    assert_eq!(downloads.load(Ordering::SeqCst), 103);
    assert!(first.path.is_file());
    execute(
        &mut service,
        Command::SetAutomatic(false),
        4 + SYNC_INTERVAL,
    );
    let requests = fixture.requests.lock().unwrap().len();
    execute(&mut service, Command::Tick, 4 + 2 * SYNC_INTERVAL);
    assert_eq!(fixture.requests.lock().unwrap().len(), requests);
}

#[test]
fn fox_rejects_private_foreign_and_mismatched_records_and_retries_failures() {
    for fault in ["private", "foreign", "mismatch", "invalid"] {
        let broken = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = broken.clone();
        let fixture = support::Fixture::new(move |r| {
            let bad = flag.load(Ordering::SeqCst);
            let response = if r.url.contains("QueryUserInfoPanel") {
                json!({"result":0,"uid":"42","username":"example","hide_game_record":if bad && fault=="private" {1}else{0}})
            } else if r.url.contains("FetchChessList") {
                let mut game = record(1);
                if bad && fault == "foreign" {
                    game["blackuid"] = json!(99);
                }
                json!({"result":0,"dstuid":"42","chesslist":[game]})
            } else {
                json!({"result":0,"chessid":if bad && fault=="mismatch" {"2"}else{"1"},"chess":if bad && fault=="invalid" {"<html>error</html>".into()}else{sgf(1)}})
            };
            (200, response.to_string())
        });
        let root = tempfile::tempdir().unwrap();
        let mut service = Service::open_for(
            root.path(),
            &fixture.endpoint,
            Arc::new(support::Vault::default()),
            Server::Fox,
        )
        .unwrap();
        assert!(
            service
                .execute(connect(), 1, &Cancellation::default(), &mut |_| {})
                .is_err(),
            "{fault}"
        );
        assert!(
            service.snapshot().games.is_empty(),
            "{fault} must not save a game"
        );
        if matches!(fault, "mismatch" | "invalid") {
            assert_eq!(service.snapshot().failures.len(), 1);
        }
        broken.store(false, Ordering::SeqCst);
        execute(&mut service, Command::SyncNow, 2);
        assert_eq!(service.snapshot().games.len(), 1);
        assert!(service.snapshot().failures.is_empty());
    }
}

#[test]
fn fox_detects_repeated_pages_instead_of_looping() {
    let fixture = support::Fixture::new(|r| {
        let response = if r.url.contains("QueryUserInfoPanel") {
            json!({"result":0,"uid":"42","username":"example"})
        } else if r.url.contains("FetchChessList") {
            json!({"result":0,"dstuid":"42","chesslist":(1..=100).map(record).collect::<Vec<_>>()})
        } else {
            let id = query(&r.url, "chessid").parse::<usize>().unwrap();
            json!({"result":0,"chessid":id.to_string(),"chess":sgf(id)})
        };
        (200, response.to_string())
    });
    let root = tempfile::tempdir().unwrap();
    let mut service = Service::open_for(
        root.path(),
        &fixture.endpoint,
        Arc::new(support::Vault::default()),
        Server::Fox,
    )
    .unwrap();
    let error = service
        .execute(connect(), 1, &Cancellation::default(), &mut |_| {})
        .unwrap_err();
    assert!(error.to_string().contains("pagination"));
    assert_eq!(service.snapshot().games.len(), 100);
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.url.contains("FetchChessList"))
            .count(),
        2
    );
}

#[test]
#[ignore = "live Fox archive: set KATASTRO_FOX_USERNAME; optional KATASTRO_FOX_ROOT keeps downloads"]
fn live_fox_archive_imports_and_reuses_public_games() {
    use katastro::pandanet::{Clock, Options, SystemClock};
    let now = SystemClock.now();
    let username = std::env::var("KATASTRO_FOX_USERNAME")
        .expect("supply a Fox account for an authorized live check");
    let temp = tempfile::tempdir().unwrap();
    let root = std::env::var_os("KATASTRO_FOX_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temp.path().into());
    let options = Options::fox(root.clone());
    let mut service =
        Service::open_for(&root, &options.endpoint, options.credentials, Server::Fox).unwrap();
    execute(
        &mut service,
        Command::Connect {
            username,
            password: String::new(),
            remember: false,
        },
        now,
    );
    let state = service.snapshot();
    assert!(!state.games.is_empty());
    assert!(state.failures.is_empty());
    for game in &state.games {
        Document::parse(&std::fs::read(&game.path).unwrap()).unwrap();
    }
    execute(&mut service, Command::SyncNow, now + 1);
    assert_eq!(service.snapshot().downloaded, 0);
    eprintln!(
        "Live Fox check: {} games parsed; repeat sync downloaded none",
        state.games.len()
    );
}
