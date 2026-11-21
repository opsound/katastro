#[path = "support/pandanet/mod.rs"]
mod support;
use katastro::{
    Document,
    archive::{Cancellation, Command, Options, Server, Service},
};
use reqwest::Url;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn connect(username: &str) -> Command {
    Command::Connect {
        username: username.into(),
        password: String::new(),
        remember: false,
    }
}
fn run(service: &mut Service, command: Command, now: u64) -> katastro::Result<()> {
    service.execute(command, now, &Cancellation::default(), &mut |_| {})
}
fn service(root: &std::path::Path, fixture: &support::Fixture) -> Service {
    Service::open_for(
        root,
        &fixture.endpoint,
        Arc::new(support::Vault::default()),
        Server::Ogs,
    )
    .unwrap()
}
fn profile() -> Value {
    json!({"count":2,"next":null,"results":[{"id":70,"username":"Example Plus"},{"id":42,"username":"Example"}]})
}
fn game(id: u64) -> Value {
    json!({"id":id,"black":42,"white":77,
        "players":{"black":{"id":42,"username":"Example"},"white":{"id":77,"username":"Rival"}},
        "mode":"game","started":"2026-01-04T12:00:00Z","ended":"2026-01-04T13:00:00.123456Z",
        "related":{"detail":"https://archive.invalid/not-the-download"}})
}
fn sgf(id: u64) -> String {
    format!("(;FF[4]GM[1]SZ[9]PB[Example]BR[2k]PW[Rival]WR[3d]RE[W+3.5]GN[Game {id}];B[cc];W[gg])")
}
fn url(path: &str) -> Url {
    Url::parse("http://localhost").unwrap().join(path).unwrap()
}

#[test]
fn an_ogs_username_at_the_unicode_character_limit_can_sync_its_game() {
    let username = format!("{}Q", "碁".repeat(127));
    let canonical = username.clone();
    let fixture = support::Fixture::new(move |r| {
        let body = match url(&r.url).path().trim_end_matches('/') {
            "/api/v1/players" => {
                json!({"count":1,"results":[{"id":42,"username":canonical}]}).to_string()
            }
            "/api/v1/players/42/games" => {
                let mut row = game(7);
                row["players"]["black"]["username"] = json!(canonical);
                json!({"count":1,"results":[row]}).to_string()
            }
            "/api/v1/games/7/sgf" => sgf(7).replace("PB[Example]", &format!("PB[{canonical}]")),
            _ => return (404, "Unknown fixture route".into()),
        };
        (200, body)
    });
    let root = tempfile::tempdir().unwrap();
    let mut archive = service(root.path(), &fixture);
    assert!(run(&mut archive, connect(&format!("{username}x")), 1).is_err());
    assert!(fixture.requests.lock().unwrap().is_empty());
    run(&mut archive, connect(&username.to_lowercase()), 2).unwrap();
    let saved = archive.snapshot();
    assert_eq!(saved.username, username);
    assert_eq!(saved.games.len(), 1);
    let doc = Document::parse(&std::fs::read(&saved.games[0].path).unwrap()).unwrap();
    assert_eq!(doc.nodes[0].property("PB"), Some(username.as_str()));
}

// Resolve an exact username even when case differs, skip unfinished games, and
// keep originals reviewable after restart without a second download.
#[test]
fn public_games_are_limited_paginated_ranked_and_reused_after_reopen() {
    let fixture = support::Fixture::new(|r| {
        let url = url(&r.url);
        assert!(r.body.is_empty());
        let query = |key: &str| {
            url.query_pairs()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.into_owned())
        };
        match url.path().trim_end_matches('/') {
            "/api/v1/players" => {
                assert!(
                    query("username__istartswith")
                        .unwrap()
                        .eq_ignore_ascii_case("Example")
                );
                let found = if query("page").as_deref() == Some("2") {
                    json!({"count":2,"next":null,"results":[{"id":42,"username":"Example"}]})
                } else {
                    json!({"count":2,"next":"?page=2","results":[{"id":70,"username":"Example Plus"}]})
                };
                (200, found.to_string())
            }
            "/api/v1/players/42/games" => {
                assert_eq!(query("ended__isnull").as_deref(), Some("false"));
                if query("ordering").as_deref() != Some("-ended") {
                    return (404, "Unsupported game ordering".into());
                }
                let body = match query("page").as_deref() {
                    Some("1") | None => {
                        let mut ongoing = game(109);
                        ongoing["ended"] = Value::Null;
                        json!({"count":4,"next":"?page=2","results":[ongoing,game(108)]})
                    }
                    Some("2") => {
                        // New finishes can shift the latest row onto the next page.
                        let mut team_game = game(107);
                        team_game["black"] = json!(55);
                        team_game["players"]["black"] = json!({"id":55,"username":"Captain"});
                        team_game["rengo_white_team"] = json!([77, null, 42]);
                        json!({"count":4,"next":null,"results":[game(108),team_game,game(106)]})
                    }
                    _ => panic!("unexpected archive page"),
                };
                (200, body.to_string())
            }
            "/api/v1/games/108/sgf" => (200, sgf(108)),
            "/api/v1/games/107/sgf" => (200, sgf(107)),
            _ => (404, "Unknown fixture route".into()),
        }
    });
    let root = tempfile::tempdir().unwrap();
    let mut archive = service(root.path(), &fixture);
    run(&mut archive, Command::SetLimit(Some(2)), 1).unwrap();
    run(&mut archive, connect("eXamPLe"), 2).unwrap();
    let state = archive.snapshot();
    assert_eq!(state.username, "Example");
    assert_eq!(state.games.len(), 2);
    let first = state.games.iter().find(|g| g.id == "108").unwrap();
    assert_eq!(std::fs::read(&first.path).unwrap(), sgf(108).as_bytes());
    assert_eq!(first.black, "Example 2k");
    assert_eq!(first.white, "Rival 3d");
    assert_eq!(first.result, "W+3.5");
    assert_eq!(first.date, "2026/01/04 13:00:00");
    assert!(state.games.iter().any(|g| g.id == "107"));
    drop(archive);
    let mut archive = service(root.path(), &fixture);
    assert_eq!(archive.snapshot().username, "Example");
    run(&mut archive, Command::Tick, 3).unwrap();
    assert_eq!(archive.snapshot().downloaded, 0);
    assert_eq!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.url.contains("/sgf"))
            .count(),
        2
    );
}

#[test]
fn incremental_sync_passes_filtered_rows_to_find_new_public_games() {
    let updated = Arc::new(AtomicBool::new(false));
    let flag = updated.clone();
    let fixture = support::Fixture::new(move |r| {
        let url = url(&r.url);
        let body = match url.path().trim_end_matches('/') {
            "/api/v1/players" => profile().to_string(),
            "/api/v1/players/42/games" if !flag.load(Ordering::SeqCst) => {
                json!({"count":1,"next":null,"results":[game(1)]}).to_string()
            }
            "/api/v1/players/42/games"
                if url.query_pairs().any(|(k, v)| k == "page" && v == "2") =>
            {
                json!({"count":4,"next":null,"results":[game(2),game(1)]}).to_string()
            }
            "/api/v1/players/42/games" => {
                let mut private = game(3);
                private["private"] = json!(true);
                let mut active = game(4);
                active["ended"] = Value::Null;
                json!({"count":4,"next":"?page=2","results":[private,active]}).to_string()
            }
            "/api/v1/games/1/sgf" => sgf(1),
            "/api/v1/games/2/sgf" => sgf(2),
            _ => return (404, "Unknown fixture route".into()),
        };
        (200, body)
    });
    let root = tempfile::tempdir().unwrap();
    let mut archive = service(root.path(), &fixture);
    run(&mut archive, connect("Example"), 1).unwrap();
    updated.store(true, Ordering::SeqCst);
    run(&mut archive, Command::SyncNow, 2).unwrap();
    assert_eq!(
        archive.snapshot().games.len(),
        2,
        "the second public game was below a filtered page"
    );
    assert_eq!(archive.snapshot().downloaded, 1);
}

#[test]
fn exact_accounts_foreign_games_and_unsafe_pagination_are_checked_before_download() {
    for fault in [
        "near_match",
        "foreign_game",
        "foreign_next",
        "repeated_page",
    ] {
        let fixture = support::Fixture::new(move |r| {
            let url = url(&r.url);
            if url.path().trim_end_matches('/') == "/api/v1/players" {
                let found = if fault == "near_match" {
                    json!({"count":1,"next":null,"results":[{"id":70,"username":"Example Plus"}]})
                } else {
                    profile()
                };
                return (200, found.to_string());
            }
            if url.path().ends_with("/sgf") {
                return (200, sgf(42));
            }
            let mut row = game(42);
            if fault == "foreign_game" {
                row["black"] = json!(55);
                row["players"]["black"] = json!({"id":55,"username":"SomeoneElse"});
            }
            let next = match fault {
                "foreign_next" => json!("https://archive.invalid/api/v1/players/42/games?page=2"),
                "repeated_page" => json!(format!(
                    "?page={}",
                    if url.query_pairs().any(|(k, v)| k == "page" && v == "2") {
                        3
                    } else {
                        2
                    }
                )),
                _ => Value::Null,
            };
            (
                200,
                json!({"count":100,"next":next,"results":[row]}).to_string(),
            )
        });
        let root = tempfile::tempdir().unwrap();
        let mut archive = service(root.path(), &fixture);
        let error = run(&mut archive, connect("Example"), 1)
            .unwrap_err()
            .to_string();
        if fault == "near_match" {
            assert!(error.contains("not found"), "{error}");
            assert_eq!(fixture.requests.lock().unwrap().len(), 1);
        } else if fault == "foreign_game" {
            assert!(error.contains("another player"), "{error}");
        } else {
            assert!(error.contains("pagination"), "{error}");
        }
        assert_eq!(
            archive.snapshot().games.len(),
            usize::from(fault == "repeated_page")
        );
    }
}

#[test]
fn unavailable_sgfs_are_retried_without_replacing_other_saved_games() {
    let broken = Arc::new(AtomicBool::new(true));
    let flag = broken.clone();
    let fixture = support::Fixture::new(move |r| {
        let path = url(&r.url);
        let body = match path.path().trim_end_matches('/') {
            "/api/v1/players" => profile().to_string(),
            "/api/v1/players/42/games" => {
                json!({"count":2,"next":null,"results":[game(3),game(2)]}).to_string()
            }
            "/api/v1/games/2/sgf" if flag.load(Ordering::SeqCst) => "<html>No game</html>".into(),
            "/api/v1/games/2/sgf" => sgf(2),
            "/api/v1/games/3/sgf" => sgf(3),
            _ => return (404, "unknown request".into()),
        };
        (200, body)
    });
    let root = tempfile::tempdir().unwrap();
    let mut archive = service(root.path(), &fixture);
    assert!(run(&mut archive, connect("Example"), 1).is_err());
    let before = archive.snapshot();
    assert_eq!(before.games.len(), 1);
    assert_eq!(before.failures.len(), 1);
    let saved = std::fs::read(&before.games[0].path).unwrap();
    broken.store(false, Ordering::SeqCst);
    run(&mut archive, Command::SyncNow, 2).unwrap();
    assert_eq!(archive.snapshot().games.len(), 2);
    assert_eq!(archive.snapshot().downloaded, 1);
    assert!(archive.snapshot().failures.is_empty());
    assert_eq!(std::fs::read(&before.games[0].path).unwrap(), saved);
}

#[test]
#[ignore = "Set KATASTRO_OGS_USERNAME to verify one public completed game against live OGS"]
fn live_ogs_imports_one_public_game() {
    let username = std::env::var("KATASTRO_OGS_USERNAME").expect("supply a public OGS username");
    let temp = tempfile::tempdir().unwrap();
    let options = Options::ogs(temp.path().into());
    let mut archive = Service::open_for(
        temp.path(),
        &options.endpoint,
        options.credentials,
        Server::Ogs,
    )
    .unwrap();
    run(&mut archive, Command::SetLimit(Some(1)), 1).unwrap();
    run(&mut archive, connect(&username), 2).unwrap();
    let saved = archive.snapshot();
    assert_eq!(saved.games.len(), 1);
    assert!(saved.username.to_lowercase() == username.to_lowercase());
    assert!(saved.failures.is_empty());
    let doc = Document::parse(&std::fs::read(&saved.games[0].path).unwrap()).unwrap();
    assert!(doc.nodes[0].property("PB").is_some() && doc.nodes[0].property("PW").is_some());
}
