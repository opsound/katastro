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
    sync: crate::pandanet::Options,
}
pub struct DesktopApp {
    pub state: Snapshot,
    pub client: Client,
    sync_client: crate::pandanet::Client,
    sync_state: crate::pandanet::Snapshot,
    sync_view: crate::pandanet::view::View,
    sync_folder: PathBuf,
    settings_open: bool,
    executable_text: String,
    model_text: String,
    dialogs: Box<dyn FileDialogs>,
    pending_dialog: Option<PendingDialog>,
    training_view: crate::training::view::View,
    training_state: crate::training::Snapshot,
    training_client: Option<crate::training::client::Client>,
    training_databases: (PathBuf, PathBuf),
    training_selection: Option<(PathBuf, usize, Vec<Option<crate::Point>>)>,
}
impl DesktopApp {
    pub fn with_sync(
        ctx: &egui::Context,
        reviews: PathBuf,
        cache: PathBuf,
        config: EngineConfig,
        initial: Option<PathBuf>,
        sync: crate::pandanet::Options,
    ) -> Self {
        Self::with_services(
            ctx,
            reviews,
            cache,
            config,
            initial,
            Services {
                dialogs: Box::new(NativeDialogs),
                sync,
            },
        )
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
        let sync = crate::pandanet::Options::new(
            reviews
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("Pandanet"),
        );
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
        let sync_folder = services.sync.root.clone();
        let sync_wake = ctx.clone();
        let sync_client = crate::pandanet::Client::spawn(
            services.sync,
            Some(Arc::new(move || sync_wake.request_repaint())),
        );
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
            settings_open: false,
            executable_text: String::new(),
            model_text: String::new(),
            dialogs: services.dialogs,
            sync_client,
            sync_state: crate::pandanet::Snapshot::default(),
            sync_view: crate::pandanet::view::View::default(),
            sync_folder,
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
                self.sync_view.show_library(&self.sync_state);
                None
            }
            Action::Training => {
                self.training_client = None;
                self.training_state = Default::default();
                self.training_view.show(&self.sync_state);
                self.sync_view.open = false;
                Some(Command::Pause)
            }
            Action::Pandanet => {
                self.sync_view.show(&self.sync_state);
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
    fn training_action(&mut self, action: crate::training::view::Action, ctx: &egui::Context) {
        use crate::training::{client, view::Action};
        match action {
            Action::Start {
                games,
                account,
                settings,
            } => {
                let _ = self.client.send(Command::Pause);
                let _ = self.client.send(Command::NumberFrom(None));
                self.training_client = None;
                self.training_selection = None;
                self.training_state = crate::training::Snapshot {
                    phase: crate::training::Phase::Searching,
                    status: "Preparing training…".into(),
                    ..Default::default()
                };
                let wake = ctx.clone();
                self.training_client = Some(client::Client::spawn(
                    client::Options::new(
                        self.training_databases.0.clone(),
                        self.training_databases.1.clone(),
                        self.state.config.clone(),
                    ),
                    games,
                    account,
                    settings,
                    Some(Arc::new(move || wake.request_repaint())),
                ));
            }
            Action::Command(command) => {
                if matches!(command, client::Command::Next) {
                    let _ = self.client.send(Command::Pause);
                    let _ = self.client.send(Command::NumberFrom(None));
                    self.training_selection = None;
                }
                if let Some(client) = &self.training_client
                    && let Err(error) = client.send(command)
                {
                    self.training_state.error = Some(error.to_string());
                }
            }
            Action::ExploreLine(line) => {
                if let Some((path, index)) = &self.training_state.reveal {
                    self.training_selection = Some((path.clone(), *index, line));
                }
            }
            Action::Back => {
                self.training_client = None;
                self.training_view.open = false;
                let _ = self.client.send(Command::NumberFrom(None));
                let _ = self.client.send(Command::PrepareTraining {
                    games: self
                        .sync_state
                        .games
                        .iter()
                        .map(|g| g.path.clone())
                        .collect(),
                    account: self.sync_state.username.clone(),
                });
            }
        }
    }
    fn poll_training(&mut self) {
        if let Some(client) = &self.training_client {
            while let Ok(state) = client.snapshots.try_recv() {
                if state.reveal != self.training_state.reveal
                    && let Some((path, index)) = &state.reveal
                {
                    self.training_selection = Some((path.clone(), *index, Vec::new()));
                    let _ = self.client.send(Command::OpenAndAnalyze(path.clone()));
                }
                self.training_state = state;
            }
        }
        if let Some((path, index, line)) = &self.training_selection
            && self.state.source.as_ref() == Some(path)
            && let Some(node) = self
                .state
                .document
                .as_ref()
                .and_then(|d| d.mainline.get(*index))
                .copied()
        {
            let _ = self.client.send(Command::NumberFrom(Some(node)));
            let _ = self.client.send(Command::Select(node));
            for point in line {
                let _ = self.client.send(Command::Play(*point));
            }
            self.training_selection = None;
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
        while let Ok(state) = self.sync_client.snapshots.try_recv() {
            if !self.training_view.open
                && (state.username != self.sync_state.username
                    || !state.games.iter().map(|g| &g.path).eq(self
                        .sync_state
                        .games
                        .iter()
                        .map(|g| &g.path)))
            {
                let _ = self.client.send(Command::PrepareTraining {
                    games: state.games.iter().map(|g| g.path.clone()).collect(),
                    account: state.username.clone(),
                });
            }
            self.sync_state = state;
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
            for action in self
                .training_view
                .render(ui, &self.training_state, &self.sync_state)
            {
                self.training_action(action, ui.ctx());
            }
        }
        if !self.training_view.open || self.training_state.phase == crate::training::Phase::Revealed
        {
            for action in ui::render(ui, &self.state) {
                self.action(action, frame);
            }
        }
        if self.settings_open {
            self.settings(ui, frame);
        }
        if self.sync_view.open {
            let actions = self
                .sync_view
                .render(ui.ctx(), &self.sync_state, &self.sync_folder);
            if let Some(command) = actions.command
                && let Err(error) = self.sync_client.send(command)
            {
                self.sync_state.error = Some(error.to_string());
            }
            if actions.train {
                self.action(Action::Training, frame);
            }
            if let Some(path) = actions.review {
                self.action(Action::Review(Command::OpenAndAnalyze(path)), frame);
            }
        }
        self.poll_dialog(ui.ctx(), frame);
    }
}
