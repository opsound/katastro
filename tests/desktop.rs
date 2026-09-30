#![cfg(feature = "desktop")]
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{Color, Point, desktop::DesktopApp, engine::EngineConfig};
use std::time::{Duration, Instant};
#[test]
fn desktop_opens_analyzes_and_saves_a_board_variation_through_the_worker() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc];W[gg];B[cg])").unwrap();
    let model = temp.path().join("model");
    std::fs::write(&model, b"fake").unwrap();
    let config = EngineConfig {
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model,
    };
    let mut harness = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 820.0))
        .build_eframe(|cc| {
            DesktopApp::new(
                &cc.egui_ctx,
                temp.path().join("reviews"),
                temp.path().join("cache"),
                config,
                Some(source),
            )
        });
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.state().state.coverage != (4, 4) {
        harness.step();
        assert!(
            Instant::now() < deadline,
            "initial SGF and automatic analysis never reached the desktop"
        );
        std::thread::yield_now();
    }
    harness.get_by_label("Next move").click();
    while harness.state().state.document.as_ref().unwrap().selected != 1 {
        harness.step();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    harness.get_by_label("Play D6").click();
    while harness.state().state.document.as_ref().unwrap().nodes.len() != 5 {
        harness.step();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        harness
            .state()
            .state
            .board
            .as_ref()
            .unwrap()
            .stone(Point::new(3, 3)),
        Some(Color::White)
    );
    assert_eq!(
        harness
            .state()
            .state
            .document
            .as_ref()
            .unwrap()
            .mainline
            .len(),
        4
    );
    // An optional local render complements the input assertions; it is not a snapshot test.
    if let Some(path) = std::env::var_os("KATASTRO_UI_PREVIEW") {
        harness.run();
        harness.render().unwrap().save(path).unwrap();
    }
}
