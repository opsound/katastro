use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{archive::Options, desktop::DesktopApp, engine::EngineConfig};
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub fn archive_desktop(
    root: &Path,
    options: Vec<Options>,
    size: egui::Vec2,
) -> Harness<'static, DesktopApp> {
    Harness::builder().with_size(size).build_eframe(|cc| {
        DesktopApp::with_archives(
            &cc.egui_ctx,
            root.join("reviews"),
            root.join("cache"),
            EngineConfig {
                executable: "missing".into(),
                model: "missing".into(),
                analysis: Default::default(),
            },
            None,
            options,
        )
    })
}

pub fn wait_for(
    h: &mut Harness<'_, DesktopApp>,
    message: &str,
    ready: impl Fn(&Harness<'_, DesktopApp>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready(h) {
        h.step();
        assert!(Instant::now() < deadline, "{message}");
        std::thread::yield_now();
    }
}

pub fn wait_for_label(h: &mut Harness<'_, DesktopApp>, label: &str) {
    wait_for(h, label, |h| h.query_by_label(label).is_some());
}

pub fn select_all_time(h: &mut Harness<'_, DesktopApp>) {
    wait_for_label(h, "Date range");
    h.get_by_label("Date range").click();
    wait_for_label(h, "All time");
    h.get_by_label("All time").click();
    h.run_steps(3);
}
