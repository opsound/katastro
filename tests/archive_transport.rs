#[path = "support/pandanet/mod.rs"]
mod support;
use katastro::archive::{Cancellation, Command, Server, Service};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

// Each archive must keep oversized or rate-limited responses out of the library.
#[test]
fn archives_bound_downloads_sanitize_errors_and_retry() {
    for server in [Server::Pandanet, Server::Fox, Server::Ogs] {
        for status in [200, 429, 503] {
            let broken = Arc::new(AtomicBool::new(true));
            let flag = broken.clone();
            let sgf = support::sgf(1);
            let large = format!("(;SZ[9]C[{}];B[cc])", "x".repeat(16 * 1024 * 1024));
            let fixture = support::Fixture::new(move |r| {
                if r.url.starts_with("/api/v1/players/42/games") {
                    return (200, json!({"count":1,"next":null,"results":[{
                        "id":1,"black":55,"white":42,
                        "players":{"black":{"id":55,"username":"opponent"},"white":{"id":42,"username":"example"}},
                        "ended":"2026-11-01T12:00:00Z"
                    }]}).to_string());
                }
                if r.url.starts_with("/api/v1/players") {
                    return (
                        200,
                        json!({"count":1,"next":null,"results":[{"id":42,"username":"example"}]})
                            .to_string(),
                    );
                }
                if r.url.contains("QueryUserInfoPanel") {
                    return (
                        200,
                        json!({"result":0,"uid":"42","username":"example"}).to_string(),
                    );
                }
                if r.url.contains("FetchChessList") {
                    return (200, json!({"result":0,"dstuid":"42","chesslist":[{"chessid":"1","blackuid":42,"whiteuid":55,"starttime":"2026-11-01"}]}).to_string());
                }
                if server == Server::Pandanet && !r.url.contains("mode=sgf") {
                    return (200, support::page(1, 1, &[1]));
                }
                let bad = flag.load(Ordering::SeqCst);
                let body = if bad && status == 200 { &large } else { &sgf };
                let body = match server {
                    Server::Pandanet | Server::Ogs => body.clone(),
                    Server::Fox => json!({"result":0,"chessid":"1","chess":body}).to_string(),
                };
                (if bad { status } else { 200 }, body)
            });
            let root = tempfile::tempdir().unwrap();
            let mut service = Service::open_for(
                root.path(),
                &fixture.endpoint,
                Arc::new(support::Vault::default()),
                server,
            )
            .unwrap();
            service
                .execute(
                    Command::Connect {
                        username: "example".into(),
                        password: "test-secret".into(),
                        remember: false,
                    },
                    1,
                    &Cancellation::default(),
                    &mut |_| {},
                )
                .unwrap_err();
            let snapshot = service.snapshot();
            assert_eq!(snapshot.failures.len(), 1);
            let error = &snapshot.failures[0].message;
            assert!(
                error.contains(if status == 200 {
                    "download limit"
                } else {
                    "HTTP"
                }),
                "{error}"
            );
            assert!(!error.contains("private-session") && !error.contains("127.0.0.1"));
            assert!(service.snapshot().games.is_empty());
            broken.store(false, Ordering::SeqCst);
            service
                .execute(Command::SyncNow, 2, &Cancellation::default(), &mut |_| {})
                .unwrap();
            assert_eq!(service.snapshot().games.len(), 1);
            assert!(service.snapshot().failures.is_empty());
            assert_eq!(
                std::fs::read(&service.snapshot().games[0].path).unwrap(),
                support::sgf(1).as_bytes()
            );
        }
    }
}
