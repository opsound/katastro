#![cfg(feature = "desktop")]
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{
    desktop::DesktopApp,
    dialogs::{DialogFuture, DialogRequest, FileDialogs},
    engine::EngineConfig,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Completion {
    value: Option<Option<PathBuf>>,
    waker: Option<Waker>,
}
#[derive(Clone, Default)]
struct ControlledDialogs {
    requests: Arc<Mutex<Vec<RecordedDialog>>>,
}
type RecordedDialog = (DialogRequest, Arc<Mutex<Completion>>);
impl ControlledDialogs {
    fn finish(&self, index: usize, path: Option<PathBuf>) {
        let pending = self.requests.lock().unwrap()[index].1.clone();
        let mut pending = pending.lock().unwrap();
        pending.value = Some(path);
        if let Some(waker) = pending.waker.take() {
            waker.wake();
        }
    }
}
impl FileDialogs for ControlledDialogs {
    fn start(&mut self, request: &DialogRequest, _parent: &eframe::Frame) -> DialogFuture {
        let completion = Arc::new(Mutex::new(Completion::default()));
        self.requests
            .lock()
            .unwrap()
            .push((request.clone(), completion.clone()));
        Box::pin(std::future::poll_fn(move |cx| {
            let mut state = completion.lock().unwrap();
            if let Some(result) = state.value.take() {
                Poll::Ready(result)
            } else {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }))
    }
}
fn wait(h: &mut Harness<'_, DesktopApp>, predicate: impl Fn(&DesktopApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate(h.state()) {
        h.step();
        assert!(
            Instant::now() < deadline,
            "dialog completion never reached the app: {} {:?}",
            h.state().state.status,
            h.state().state.error
        );
        std::thread::yield_now();
    }
}

#[test]
fn open_dialog_delivers_delayed_selection_and_cancel_preserves_the_review() {
    let temp = tempfile::TempDir::new().unwrap();
    let sgf = temp.path().join("selected.sgf");
    std::fs::write(&sgf, b"(;SZ[9];B[cc];W[gg])").unwrap();
    let sgf = std::fs::canonicalize(sgf).unwrap();
    let dialogs = ControlledDialogs::default();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 820.0))
        .build_eframe(|cc| {
            DesktopApp::with_dialogs(
                &cc.egui_ctx,
                temp.path().join("reviews"),
                temp.path().join("cache"),
                EngineConfig {
                    executable: "missing".into(),
                    model: "missing".into(),
                },
                None,
                Box::new(dialogs.clone()),
            )
        });
    // A genuine toolbar click starts a pending dialog; render callbacks must keep returning.
    h.get_all_by_label("Open SGF").next().unwrap().click();
    h.run();
    assert_eq!(
        dialogs.requests.lock().unwrap()[0].0,
        DialogRequest::OpenGame
    );
    assert!(h.state().state.document.is_none());
    h.get_all_by_label("Open SGF").next().unwrap().click();
    h.run();
    assert_eq!(
        dialogs.requests.lock().unwrap().len(),
        1,
        "repeated input must not stack native sheets"
    );
    dialogs.finish(0, Some(sgf.clone()));
    wait(&mut h, |app| app.state.source.as_ref() == Some(&sgf));
    assert_eq!(h.state().state.document.as_ref().unwrap().mainline.len(), 3);
    h.get_by_label("Next move").click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().selected == 1
    });
    h.get_by_label("Open SGF").click();
    h.run();
    // The render callback and worker snapshots keep progressing while the choice is outstanding.
    h.get_by_label("Next move").click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().selected == 2
    });
    let before = h.state().state.document.clone().unwrap().to_sgf();
    dialogs.finish(1, None);
    h.run();
    assert_eq!(h.state().state.source.as_ref(), Some(&sgf));
    assert_eq!(h.state().state.document.as_ref().unwrap().selected, 2);
    assert_eq!(h.state().state.document.as_ref().unwrap().to_sgf(), before);
    assert!(h.state().state.error.is_none());
}

#[test]
fn export_dialog_waits_for_selection_and_exports_saved_variations() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    let bytes = b"(;SZ[9];B[cc];W[gg])";
    std::fs::write(&source, bytes).unwrap();
    let target = temp.path().join("review.sgf");
    let dialogs = ControlledDialogs::default();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 820.0))
        .build_eframe(|cc| {
            DesktopApp::with_dialogs(
                &cc.egui_ctx,
                temp.path().join("reviews"),
                temp.path().join("cache"),
                EngineConfig {
                    executable: "missing".into(),
                    model: "missing".into(),
                },
                Some(source.clone()),
                Box::new(dialogs.clone()),
            )
        });
    wait(&mut h, |app| app.state.document.is_some());
    h.get_by_label("Next move").click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().selected == 1
    });
    h.get_by_label("Play D6").click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().nodes.len() == 4
    });
    h.get_by_label("Export SGF").click();
    h.run();
    assert_eq!(
        dialogs.requests.lock().unwrap()[0].0,
        DialogRequest::ExportGame {
            filename: "game.review.sgf".into()
        }
    );
    assert!(!target.exists());
    dialogs.finish(0, Some(target.clone()));
    wait(&mut h, |app| app.state.status.starts_with("Exported "));
    let exported = katastro::Document::parse(&std::fs::read(target).unwrap()).unwrap();
    assert_eq!(exported.nodes.len(), 4);
    assert_eq!(exported.mainline.len(), 3);
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn engine_file_dialogs_deliver_selections_to_saved_settings() {
    let temp = tempfile::TempDir::new().unwrap();
    let executable = temp.path().join("chosen-engine");
    let model = temp.path().join("chosen-model.bin.gz");
    let dialogs = ControlledDialogs::default();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 820.0))
        .build_eframe(|cc| {
            DesktopApp::with_dialogs(
                &cc.egui_ctx,
                temp.path().join("reviews"),
                temp.path().join("cache"),
                EngineConfig {
                    executable: "missing".into(),
                    model: "missing".into(),
                },
                None,
                Box::new(dialogs.clone()),
            )
        });
    h.get_by_label("Engine settings").click();
    h.run();
    h.get_by_label("Choose model").click();
    h.run();
    assert_eq!(
        dialogs.requests.lock().unwrap()[0].0,
        DialogRequest::EngineModel
    );
    dialogs.finish(0, Some(model.clone()));
    h.run();
    h.get_by_label("Choose executable").click();
    h.run();
    assert_eq!(
        dialogs.requests.lock().unwrap()[1].0,
        DialogRequest::EngineExecutable
    );
    dialogs.finish(1, Some(executable.clone()));
    h.run();
    h.get_by_label("Save engine settings").click();
    wait(&mut h, |app| {
        app.state.config.executable == executable && app.state.config.model == model
    });
    assert!(h.state().state.error.is_none());
}
