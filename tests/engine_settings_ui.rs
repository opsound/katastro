#![cfg(feature = "desktop")]
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use katastro::{desktop::DesktopApp, engine::EngineConfig, worker::Command};
use std::time::{Duration, Instant};
fn wait(h: &mut Harness<'_, DesktopApp>, predicate: impl Fn(&DesktopApp) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !predicate(h.state()) {
        h.step();
        assert!(
            h.state().state.error.is_none(),
            "{:?}",
            h.state().state.error
        );
        assert!(
            Instant::now() < end,
            "settings action did not finish: {} {:?}",
            h.state().state.status,
            h.state().state.config.analysis
        );
        std::thread::yield_now();
    }
}
fn number(h: &mut Harness<'_, DesktopApp>, label: &str, value: &str) {
    h.get_by_label(label).click();
    h.run();
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::A);
    h.get_by_label(label).type_text(value);
    h.key_press(eframe::egui::Key::Enter);
    h.run();
}

// Actual settings input must control review work and survive application restart.
// Reset restores analysis defaults while preserving the chosen engine/model.
#[test]
fn settings_controls_save_drive_analysis_reopen_and_restore_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let model = temp.path().join("model");
    let source = temp.path().join("game.sgf");
    std::fs::write(&model, b"settings-ui-fixture").unwrap();
    std::fs::write(&source, b"(;SZ[9];B[cc];W[gg])").unwrap();
    let cfg = EngineConfig {
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model: model.clone(),
        analysis: Default::default(),
    };
    let build = |cfg| {
        Harness::builder()
            .with_size(eframe::egui::vec2(1200.0, 820.0))
            .build_eframe(|cc| {
                DesktopApp::new(
                    &cc.egui_ctx,
                    temp.path().join("reviews"),
                    temp.path().join("cache"),
                    cfg,
                    None,
                )
            })
    };
    let mut h = build(cfg);
    h.get_by_label("Engine settings").click();
    h.run();
    h.get_by_label("Limit review analysis").click();
    h.run();
    number(&mut h, "Visits per position", "100");
    number(&mut h, "Parallel positions", "2");
    number(&mut h, "Threads per position", "3");
    h.get_by_label("Save engine settings").scroll_to_me();
    h.run();
    h.get_by_label("Save engine settings").click();
    wait(&mut h, |app| {
        app.state.config.analysis.max_visits == Some(100)
    });
    assert_eq!(h.state().state.config.analysis.parallel_positions, 2);
    assert_eq!(h.state().state.config.analysis.threads_per_position, 3);
    h.state()
        .client
        .send(Command::OpenAndAnalyze(source))
        .unwrap();
    wait(&mut h, |app| {
        app.state.coverage == (3, 3)
            && !app.state.running
            && app.state.values.values().all(|a| a.visits == 100)
    });
    drop(h);
    let mut h = build(EngineConfig::default());
    wait(&mut h, |app| {
        app.state.config.analysis.max_visits == Some(100)
    });
    h.get_by_label("Engine settings").click();
    h.run();
    assert_eq!(
        h.get_by_label("Visits per position")
            .accesskit_node()
            .numeric_value(),
        Some(100.0)
    );
    assert_eq!(
        h.get_by_label("Parallel positions")
            .accesskit_node()
            .numeric_value(),
        Some(2.0)
    );
    assert_eq!(
        h.get_by_label("Threads per position")
            .accesskit_node()
            .numeric_value(),
        Some(3.0)
    );
    if let Some(path) = std::env::var_os("KATASTRO_ENGINE_SETTINGS_PREVIEW") {
        h.render()
            .unwrap()
            .save(std::path::PathBuf::from(path))
            .unwrap();
    }
    h.get_by_label("Restore analysis defaults").click();
    h.run();
    h.get_by_label("Save engine settings").scroll_to_me();
    h.run();
    h.get_by_label("Save engine settings").click();
    wait(&mut h, |app| {
        app.state.config.analysis == Default::default()
    });
    assert_eq!(h.state().state.config.model, model);
}
