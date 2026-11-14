//! Bridge training attempts and revealed lines to the existing review worker.
use super::*;
impl DesktopApp {
    pub(super) fn training_action(
        &mut self,
        action: crate::training::view::Action,
        ctx: &egui::Context,
    ) {
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
    pub(super) fn poll_training(&mut self) {
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
}
