//! Desktop coordination for independently synchronized game archives.
use crate::{
    archive::{self, Server, Snapshot},
    ui::Action,
    worker::Command,
};
use eframe::egui;
use std::{path::PathBuf, sync::Arc};
mod categories;

struct Archive {
    client: archive::Client,
    state: Snapshot,
    folder: PathBuf,
    pending_games: bool,
}
pub(super) struct Library {
    pub view: archive::view::View,
    archives: Vec<Archive>,
    scanner: categories::Scanner,
    scanning: bool,
    rescan: bool,
}
impl Library {
    pub fn new(
        options: Vec<archive::Options>,
        ctx: &egui::Context,
        databases: (PathBuf, PathBuf),
    ) -> Self {
        let archives = options
            .into_iter()
            .map(|options| {
                let folder = options.root.clone();
                let state = Snapshot {
                    server: options.server,
                    ..Default::default()
                };
                let ctx = ctx.clone();
                let client =
                    archive::Client::spawn(options, Some(Arc::new(move || ctx.request_repaint())));
                Archive {
                    client,
                    state,
                    folder,
                    pending_games: false,
                }
            })
            .collect();
        Self {
            view: Default::default(),
            archives,
            scanner: categories::Scanner::new(databases, ctx.clone()),
            scanning: false,
            rescan: false,
        }
    }
    pub fn states(&self) -> Vec<&Snapshot> {
        self.archives.iter().map(|a| &a.state).collect()
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for archive in &mut self.archives {
            while let Ok(state) = archive.client.snapshots.try_recv() {
                fn entry(g: &archive::Game) -> (&std::path::Path, &str, &str, &str) {
                    (&g.path, &g.black, &g.white, &g.result)
                }
                let account_changed = state.username != archive.state.username;
                let games_changed = !state.games.iter().map(entry).eq(archive
                    .state
                    .games
                    .iter()
                    .map(entry));
                changed |= account_changed || games_changed;
                // A repair downloads a missing original into its existing archive row.
                archive.pending_games |=
                    games_changed || state.downloaded > archive.state.downloaded;
                if account_changed || (!state.syncing && archive.pending_games) {
                    self.rescan = true;
                    archive.pending_games = false;
                }
                archive.state = state;
            }
        }
        changed
    }
    pub fn categories(
        &mut self,
        state: &crate::worker::Snapshot,
        config_changed: bool,
        visible: bool,
    ) {
        let open =
            visible && self.view.open && self.archives.iter().any(|a| !a.state.games.is_empty());
        if open != self.scanning || (open && (self.rescan || config_changed)) {
            self.view.categories.clear();
            self.rescan = false;
            self.scanning = open;
            self.scanner.request(open.then(|| {
                let rows = self
                    .archives
                    .iter()
                    .map(|a| (a.state.username.clone(), a.state.games.clone()))
                    .collect();
                (state.config.clone(), rows)
            }));
        }
        if open {
            self.scanner
                .collect(&mut self.view.categories, state.source.as_deref());
        }
    }
    pub fn show_settings(&mut self, server: Server) {
        if let Some(archive) = self.archives.iter().find(|a| a.state.server == server) {
            self.view.show(&archive.state);
        }
    }
    pub fn training_default(&self) -> Option<&Snapshot> {
        let selected = self.view.training_server();
        self.archives
            .iter()
            .find(|a| a.state.server == selected && !a.state.games.is_empty())
            .or_else(|| self.archives.iter().find(|a| !a.state.games.is_empty()))
            .or_else(|| self.archives.first())
            .map(|a| &a.state)
    }
    pub fn prepare(&self) -> Command {
        Command::PrepareTrainingLibraries(
            self.archives
                .iter()
                .map(|a| &a.state)
                .filter(|s| !s.username.is_empty())
                .map(|s| {
                    (
                        s.username.clone(),
                        s.games.iter().map(|g| g.path.clone()).collect(),
                    )
                })
                .collect(),
        )
    }
    pub fn render(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let before = self.view.statistics;
        let actions = self.view.render(
            ctx,
            &self
                .archives
                .iter()
                .map(|a| (&a.state, a.folder.as_path()))
                .collect::<Vec<_>>(),
        );
        if let Some((server, command)) = actions.command
            && let Some(archive) = self.archives.iter_mut().find(|a| a.state.server == server)
            && let Err(error) = archive.client.send(command)
        {
            archive.state.error = Some(error.to_string());
        }
        let mut out = Vec::new();
        if let Some(settings) = self.view.statistics
            && Some(settings) != before
        {
            out.push(Action::Review(Command::SaveGameStatistics(settings)));
        }
        if actions.train {
            out.push(Action::Training);
        }
        if let Some(path) = actions.review {
            out.push(Action::Review(Command::OpenAndAnalyze(path)));
        }
        out
    }
}
