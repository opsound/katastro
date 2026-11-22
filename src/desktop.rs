use crate::{
    dialogs::{DialogFuture, DialogRequest, FileDialogs, NativeDialogs},
    engine::EngineConfig,
    ui::{self, Action},
    worker::{Client, Command, Snapshot},
};
use eframe::egui;
#[cfg(test)]
mod tests;
use std::{
    path::PathBuf,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
mod library;
mod opponent;
mod settings;
mod training;
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
struct Services {
    dialogs: Box<dyn FileDialogs>,
    sync: Vec<crate::archive::Options>,
}
fn default_archives(folder: &std::path::Path) -> Vec<crate::archive::Options> {
    use crate::archive::Options;
    vec![
        Options::new(folder.join("Pandanet")),
        Options::fox(folder.join("Fox")),
        Options::ogs(folder.join("OGS")),
    ]
}
pub struct DesktopApp {
    pub state: Snapshot,
    pub client: Client,
    library: library::Library,
    settings: settings::Settings,
    opponent: Option<opponent::OpponentView>,
    dialogs: Box<dyn FileDialogs>,
    pending_dialog: Option<PendingDialog>,
    training_view: crate::training::view::View,
    training_state: crate::training::Snapshot,
    training_client: Option<crate::training::client::Client>,
    training_databases: (PathBuf, PathBuf),
    training_selection: Option<(PathBuf, usize, Vec<Option<crate::Point>>)>,
}
impl DesktopApp {
    pub fn with_archives(
        ctx: &egui::Context,
        reviews: PathBuf,
        cache: PathBuf,
        config: EngineConfig,
        initial: Option<PathBuf>,
        archives: Vec<crate::archive::Options>,
    ) -> Self {
        Self::with_services(
            ctx,
            reviews,
            cache,
            config,
            initial,
            Services {
                dialogs: Box::new(NativeDialogs),
                sync: archives,
            },
        )
    }
    pub fn with_sync(
        ctx: &egui::Context,
        reviews: PathBuf,
        cache: PathBuf,
        config: EngineConfig,
        initial: Option<PathBuf>,
        sync: crate::archive::Options,
    ) -> Self {
        let folder = sync.root.parent().unwrap_or(std::path::Path::new("."));
        let mut archives = default_archives(folder);
        archives.retain(|archive| archive.server != sync.server);
        archives.insert(0, sync);
        Self::with_archives(ctx, reviews, cache, config, initial, archives)
    }
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
        let folder = reviews.parent().unwrap_or(std::path::Path::new("."));
        let sync = default_archives(folder);
        Self::with_services(
            ctx,
            reviews,
            cache,
            config,
            initial,
            Services { dialogs, sync },
        )
    }
    fn with_services(
        ctx: &egui::Context,
        reviews: PathBuf,
        cache: PathBuf,
        config: EngineConfig,
        initial: Option<PathBuf>,
        services: Services,
    ) -> Self {
        ctx.set_visuals(egui::Visuals::dark());
        ctx.global_style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
            style.visuals.selection.bg_fill = egui::Color32::from_rgb(53, 109, 94);
            style.visuals.panel_fill = egui::Color32::from_rgb(26, 30, 36);
        });
        let training_databases = (reviews.clone(), cache.clone());
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
            settings: Default::default(),
            opponent: None,
            dialogs: services.dialogs,
            library: library::Library::new(services.sync, ctx, training_databases.clone()),
            pending_dialog: None,
            training_view: Default::default(),
            training_state: Default::default(),
            training_client: None,
            training_databases,
            training_selection: None,
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
            Action::Games => {
                self.library.view.show_library();
                None
            }
            Action::PlayAi => {
                self.opponent = Some(Default::default());
                Some(Command::Pause)
            }
            Action::Training => {
                self.training_client = None;
                self.training_state = Default::default();
                if let Some(state) = self.library.training_default() {
                    self.training_view.show(state);
                }
                self.library.view.open = false;
                Some(Command::Pause)
            }
            Action::Pandanet => {
                self.library.show_settings(crate::archive::Server::Pandanet);
                None
            }
            Action::Settings => {
                self.settings.show(&self.state.config);
                None
            }
        };
        if let Some(command) = command
            && let Err(error) = self.client.send(command)
        {
            self.state.error = Some(error.to_string());
        }
    }
    fn prepare_libraries(&self) {
        let _ = self.client.send(self.library.prepare());
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
                self.settings.executable = path.to_string_lossy().into_owned()
            }
            DialogRequest::EngineModel => self.settings.model = path.to_string_lossy().into_owned(),
        }
    }
}
impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let mut config_changed = false;
        while let Ok(state) = self.client.snapshots.try_recv() {
            config_changed |= state.config.executable != self.state.config.executable
                || state.config.model != self.state.config.model;
            self.state = state;
        }
        let library_changed = self.library.poll();
        if library_changed && !self.training_view.open && self.opponent.is_none() {
            self.prepare_libraries();
        }
        if let Some(view) = &mut self.opponent {
            self.library.categories(&self.state, config_changed, false);
            if let Some(action) = view.render(
                ui,
                &self.state.config,
                self.training_databases
                    .0
                    .parent()
                    .unwrap_or(std::path::Path::new(".")),
            ) {
                self.opponent = None;
                if let opponent::Action::Review(path) = action {
                    let _ = self.client.send(Command::OpenAndAnalyze(path));
                }
                self.prepare_libraries();
                ui.ctx().request_repaint();
            }
            return;
        }
        self.poll_training();
        let dropped = ui.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .next()
        });
        if let Some(path) = dropped
            && !self.training_view.open
        {
            self.action(Action::Review(Command::OpenAndAnalyze(path)), frame);
        }
        if self.training_view.open {
            for action in self.training_view.render_libraries(
                ui,
                &self.training_state,
                &self.library.states(),
            ) {
                self.training_action(action, ui.ctx());
            }
        }
        if !self.training_view.open || self.training_state.phase == crate::training::Phase::Revealed
        {
            for action in ui::render(ui, &self.state) {
                self.action(action, frame);
            }
        }
        if self.settings.open {
            self.settings(ui, frame);
        }
        self.library.categories(&self.state, config_changed, true);
        if self.library.view.open {
            for action in self.library.render(ui.ctx()) {
                self.action(action, frame);
            }
        }
        self.poll_dialog(ui.ctx(), frame);
    }
}
