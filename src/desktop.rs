use crate::{
    dialogs::{DialogFuture, DialogRequest, FileDialogs, NativeDialogs},
    engine::EngineConfig,
    ui::{self, Action},
    worker::{Client, Command, Snapshot},
};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
struct PendingDialog {
    request: DialogRequest,
    future: DialogFuture,
}
struct DialogWake(egui::Context);
impl Wake for DialogWake {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.request_repaint();
    }
}
pub struct DesktopApp {
    pub state: Snapshot,
    pub client: Client,
    settings_open: bool,
    executable_text: String,
    model_text: String,
    dialogs: Box<dyn FileDialogs>,
    pending_dialog: Option<PendingDialog>,
}
impl DesktopApp {
    pub fn new(
        ctx: &egui::Context,
        reviews: PathBuf,
        cache: PathBuf,
        config: EngineConfig,
        initial: Option<PathBuf>,
    ) -> Self {
        Self::with_dialogs(
            ctx,
            reviews,
            cache,
            config,
            initial,
            Box::new(NativeDialogs),
        )
    }
    pub fn with_dialogs(
        ctx: &egui::Context,
        reviews: PathBuf,
        cache: PathBuf,
        config: EngineConfig,
        initial: Option<PathBuf>,
        dialogs: Box<dyn FileDialogs>,
    ) -> Self {
        ctx.set_visuals(egui::Visuals::dark());
        ctx.global_style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
            style.visuals.selection.bg_fill = egui::Color32::from_rgb(53, 109, 94);
            style.visuals.panel_fill = egui::Color32::from_rgb(26, 30, 36);
        });
        let wake = ctx.clone();
        let client = Client::spawn_with_wake(
            reviews,
            cache,
            config.clone(),
            Some(Arc::new(move || wake.request_repaint())),
        );
        if let Some(path) = initial {
            let _ = client.send(Command::OpenAndAnalyze(path));
        }
        Self {
            state: Snapshot::empty(config),
            client,
            settings_open: false,
            executable_text: String::new(),
            model_text: String::new(),
            dialogs,
            pending_dialog: None,
        }
    }
    fn action(&mut self, action: Action, frame: &eframe::Frame) {
        let command = match action {
            Action::Review(command) => Some(command),
            Action::Open => {
                self.begin_dialog(DialogRequest::OpenGame, frame);
                None
            }
            Action::Export => {
                let filename = self
                    .state
                    .source
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|stem| format!("{}.review.sgf", stem.to_string_lossy()))
                    .unwrap_or("review.sgf".into());
                self.begin_dialog(DialogRequest::ExportGame { filename }, frame);
                None
            }
            Action::Settings => {
                self.executable_text = self.state.config.executable.to_string_lossy().into_owned();
                self.model_text = self.state.config.model.to_string_lossy().into_owned();
                self.settings_open = true;
                None
            }
        };
        if let Some(command) = command
            && let Err(error) = self.client.send(command)
        {
            self.state.error = Some(error.to_string());
        }
    }
    fn begin_dialog(&mut self, request: DialogRequest, frame: &eframe::Frame) {
        if self.pending_dialog.is_none() {
            let future = self.dialogs.start(&request, frame);
            self.pending_dialog = Some(PendingDialog { request, future });
        }
    }
    fn poll_dialog(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(pending) = &mut self.pending_dialog else {
            return;
        };
        let waker = Waker::from(Arc::new(DialogWake(ctx.clone())));
        let mut cx = Context::from_waker(&waker);
        let Poll::Ready(path) = pending.future.as_mut().poll(&mut cx) else {
            return;
        };
        let request = self.pending_dialog.take().unwrap().request;
        let Some(path) = path else {
            return;
        };
        match request {
            DialogRequest::OpenGame => {
                self.action(Action::Review(Command::OpenAndAnalyze(path)), frame)
            }
            DialogRequest::ExportGame { .. } => {
                self.action(Action::Review(Command::Export(path)), frame)
            }
            DialogRequest::EngineExecutable => {
                self.executable_text = path.to_string_lossy().into_owned()
            }
            DialogRequest::EngineModel => self.model_text = path.to_string_lossy().into_owned(),
        }
    }
    fn settings(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let mut open = self.settings_open;
        let mut save = false;
        let mut choose = None;
        egui::Window::new("KataGo engine")
            .open(&mut open)
            .resizable(false)
            .default_width(540.0)
            .show(ui.ctx(), |ui| {
                ui.label("Analysis runs locally. Your SGFs and reviews stay on this Mac.");
                ui.add_space(12.0);
                ui.label("KataGo executable");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.executable_text).desired_width(410.0));
                    if ui.button("Choose executable").clicked() {
                        choose = Some(DialogRequest::EngineExecutable);
                    }
                });
                ui.label("Neural network model (.bin.gz)");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.model_text).desired_width(410.0));
                    if ui.button("Choose model").clicked() {
                        choose = Some(DialogRequest::EngineModel);
                    }
                });
                ui.add_space(12.0);
                ui.label("The app fills the chart at one visit per position, then refines to 8 and 64 visits. Pause at any time; each result is cached.");
                ui.add_space(12.0);
                if ui.button("Save engine settings").clicked() {
                    save = true;
                }
                if let Some(diagnostic) = &self.state.diagnostic {
                    ui.collapsing("Latest engine message", |ui| { ui.label(diagnostic); });
                }
            });
        self.settings_open = open;
        if let Some(request) = choose {
            self.begin_dialog(request, frame);
        }
        if save {
            self.action(
                Action::Review(Command::Configure(EngineConfig {
                    executable: self.executable_text.trim().into(),
                    model: self.model_text.trim().into(),
                })),
                frame,
            );
            self.settings_open = false;
        }
    }
}
impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        while let Ok(state) = self.client.snapshots.try_recv() {
            self.state = state;
        }
        let dropped = ui.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .next()
        });
        if let Some(path) = dropped {
            self.action(Action::Review(Command::OpenAndAnalyze(path)), frame);
        }
        for action in ui::render(ui, &self.state) {
            self.action(action, frame);
        }
        if self.settings_open {
            self.settings(ui, frame);
        }
        self.poll_dialog(ui.ctx(), frame);
    }
}
