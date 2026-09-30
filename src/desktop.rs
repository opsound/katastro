use crate::{
    engine::EngineConfig,
    ui::{self, Action},
    worker::{Client, Command, Snapshot},
};
use eframe::egui;
use std::{path::PathBuf, sync::Arc};
pub struct DesktopApp {
    pub state: Snapshot,
    pub client: Client,
    settings_open: bool,
    executable_text: String,
    model_text: String,
}
impl DesktopApp {
    pub fn new(
        ctx: &egui::Context,
        reviews: PathBuf,
        cache: PathBuf,
        config: EngineConfig,
        initial: Option<PathBuf>,
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
        }
    }
    fn action(&mut self, action: Action) {
        let command = match action {
            Action::Review(command) => Some(command),
            Action::Open => rfd::FileDialog::new()
                .add_filter("Go game", &["sgf"])
                .pick_file()
                .map(Command::OpenAndAnalyze),
            Action::Export => {
                let filename = self
                    .state
                    .source
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|stem| format!("{}.review.sgf", stem.to_string_lossy()))
                    .unwrap_or("review.sgf".into());
                rfd::FileDialog::new()
                    .add_filter("Go game", &["sgf"])
                    .set_file_name(filename)
                    .save_file()
                    .map(Command::Export)
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
    fn settings(&mut self, ui: &mut egui::Ui) {
        let mut open = self.settings_open;
        let mut save = false;
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
                    if ui.button("Choose executable").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_file()
                    {
                        self.executable_text = path.to_string_lossy().into_owned();
                    }
                });
                ui.label("Neural network model (.bin.gz)");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.model_text).desired_width(410.0));
                    if ui.button("Choose model").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("KataGo model", &["gz"])
                            .pick_file()
                    {
                        self.model_text = path.to_string_lossy().into_owned();
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
        if save {
            self.action(Action::Review(Command::Configure(EngineConfig {
                executable: self.executable_text.trim().into(),
                model: self.model_text.trim().into(),
            })));
            self.settings_open = false;
        }
    }
}
impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
            self.action(Action::Review(Command::OpenAndAnalyze(path)));
        }
        for action in ui::render(ui, &self.state) {
            self.action(action);
        }
        if self.settings_open {
            self.settings(ui);
        }
    }
}
