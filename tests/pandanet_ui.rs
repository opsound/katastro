#![cfg(feature = "desktop")]
#[path = "support/pandanet/mod.rs"]
mod support;
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{
    desktop::DesktopApp,
    engine::EngineConfig,
    pandanet::{Options, SystemClock},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
fn wait(h: &mut Harness<'_, DesktopApp>, done: impl Fn(&DesktopApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(h.state()) {
        h.step();
        assert!(
            Instant::now() < deadline,
            "desktop did not finish the requested operation"
        );
        std::thread::yield_now();
    }
}
#[test]
fn desktop_signs_in_remembers_downloads_and_opens_a_game_without_disturbing_a_review() {
    let (entered, arrived) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let fixture = support::Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(10)).unwrap();
            (200, support::sgf(1))
        } else {
            (200, support::page(1, 1, &[1]))
        }
    });
    struct Release(std::sync::mpsc::Sender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let release = Release(release);
    let temp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let source = temp.path().join("original.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc];W[gg])").unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200., 860.))
        .build_eframe(|cc| {
            DesktopApp::with_sync(
                &cc.egui_ctx,
                temp.path().join("reviews.sqlite"),
                temp.path().join("cache.sqlite"),
                EngineConfig {
                    executable: "missing".into(),
                    model: "missing".into(),
                },
                Some(source),
                Options {
                    root: temp.path().join("Pandanet"),
                    endpoint: fixture.endpoint.clone(),
                    request_interval: Duration::ZERO,
                    credentials: vault.clone(),
                    clock: Arc::new(SystemClock),
                },
            )
        });
    wait(&mut h, |app| app.state.document.is_some());
    h.get_by_label("Pandanet").click();
    h.run_steps(3);
    h.get_by_label("Pandanet username").focus();
    h.get_by_label("Pandanet username").type_text("example");
    h.get_by_label("Pandanet password").focus();
    h.get_by_label("Pandanet password").type_text("test-secret");
    h.run_steps(3);
    h.get_by_label("Remember login in Keychain").click();
    h.run_steps(3);
    h.get_by_label("Connect and sync").click();
    h.run_steps(3);
    arrived
        .recv_timeout(Duration::from_secs(3))
        .expect("desktop must start the download");
    // The server holds a real request pending while the user navigates the board.
    h.get_by_label("Close Pandanet").click();
    h.run_steps(3);
    h.get_by_label("Next move").click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().selected == 1
    });
    h.get_by_label("Play D6").click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().nodes.len() == 4
    });
    release.0.send(()).unwrap();
    h.get_by_label("Pandanet").click();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h
        .query_by_label("Review 2026/11/01 12:00: example 6k+ vs opponent 5k+")
        .is_none()
    {
        h.step();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes.len(),
        4,
        "sync must preserve the open review"
    );
    h.get_by_label("Review 2026/11/01 12:00: example 6k+ vs opponent 5k+")
        .click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().mainline.len() == 4
    });
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes[0].property("PW"),
        Some("example")
    );
    assert_eq!(
        vault.0.lock().unwrap().get("example").map(String::as_str),
        Some("test-secret")
    );
}

#[test]
fn games_library_filters_synced_games_and_opens_the_selected_record_directly() {
    use katastro::pandanet::{Cancellation, Command};
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (
                200,
                support::sgf(if r.url.contains("game2") { 2 } else { 1 }),
            )
        } else {
            (200, support::page(1, 1, &[2, 1]))
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Pandanet");
    let vault = Arc::new(support::Vault::default());
    let mut service = fixture.service(&root, vault.clone());
    service
        .execute(
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
    service
        .execute(
            Command::SetAutomatic(false),
            101,
            &Cancellation::default(),
            &mut |_| {},
        )
        .unwrap();
    drop(service);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200., 860.))
        .build_eframe(|cc| {
            DesktopApp::with_sync(
                &cc.egui_ctx,
                temp.path().join("reviews.sqlite"),
                temp.path().join("cache.sqlite"),
                EngineConfig {
                    executable: "missing".into(),
                    model: "missing".into(),
                },
                None,
                Options {
                    root,
                    endpoint: fixture.endpoint.clone(),
                    request_interval: Duration::ZERO,
                    credentials: vault,
                    clock: Arc::new(SystemClock),
                },
            )
        });
    h.get_by_label("Games").click();
    h.run_steps(3);
    let first = "Review 2026/11/01 12:00: example 6k+ vs opponent 5k+";
    let second = "Review 2026/11/02 12:00: example 6k+ vs opponent 5k+";
    let deadline = Instant::now() + Duration::from_secs(3);
    while h.query_by_label(second).is_none() {
        h.step();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    if let Some(path) = std::env::var_os("KATASTRO_PANDANET_UI_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    assert!(h.query_by_label(first).is_some());
    assert!(
        h.query_by_label("Pandanet password").is_none(),
        "browsing games should not require opening account settings"
    );
    h.get_by_label("Search synced games").focus();
    h.get_by_label("Search synced games")
        .type_text("2026/11/01");
    h.run_steps(3);
    assert!(
        h.query_by_label(second).is_none(),
        "search must filter the game list"
    );
    h.get_by_label(first).click();
    wait(&mut h, |app| app.state.document.is_some());
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes[0].property("DT"),
        Some("2026-11-01")
    );
    assert_eq!(h.state().state.document.as_ref().unwrap().mainline.len(), 4);
}
