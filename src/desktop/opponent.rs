use crate::{
    Color,
    engine::EngineConfig,
    opponent::{Client, Input, RANKS, Settings, State},
};
use eframe::egui;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct OpponentView {
    settings: Settings,
    client: Option<Client>,
    state: Option<State>,
    error: Option<String>,
}
pub(super) enum Action {
    Close,
    Review(PathBuf),
}

impl OpponentView {
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        config: &EngineConfig,
        data: &Path,
    ) -> Option<Action> {
        if self.client.is_none() {
            let mut action = None;
            egui::CentralPanel::default().show(ui, |ui| {
                ui.heading("Play against AI");
                ui.label("KataGo imitates moves played by humans of the chosen rank.");
                ui.horizontal(|ui| {
                    let label = ui.label("Opponent rank");
                    egui::ComboBox::from_id_salt("opponent-rank")
                        .selected_text(self.settings.rank)
                        .height(620.)
                        .show_ui(ui, |ui| {
                            for rank in RANKS {
                                ui.selectable_value(&mut self.settings.rank, rank, rank);
                            }
                        })
                        .response
                        .labelled_by(label.id);
                });
                ui.horizontal(|ui| {
                    ui.label("Your stones");
                    for color in [Color::Black, Color::White] {
                        ui.selectable_value(&mut self.settings.player, color, color.to_string());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Board size");
                    for size in [9, 13, 19] {
                        ui.selectable_value(
                            &mut self.settings.size,
                            size,
                            format!("{size}×{size}"),
                        );
                    }
                });
                ui.label(
                    "Chinese area scoring, 7.5 komi. Capture dead stones before both players pass.",
                );
                if ui.button("Start game").clicked() {
                    let ctx = ui.ctx().clone();
                    if let Some(human) = crate::runtime::human_model() {
                        self.error = None;
                        self.client = Some(Client::start(
                            self.settings,
                            config.clone(),
                            data.join("Played Games"),
                            human,
                            Some(Arc::new(move || ctx.request_repaint())),
                        ));
                    } else {
                        self.error = Some("The bundled HumanSL model is unavailable".into());
                    }
                }
                if ui.button("Back to review").clicked() {
                    action = Some(Action::Close);
                }
                if let Some(e) = &self.error {
                    ui.label(e);
                }
            });
            return action;
        }
        let client = self.client.as_ref().unwrap();
        while let Ok(update) = client.states.try_recv() {
            match update {
                Ok(s) => self.state = Some(s),
                Err(e) => self.error = Some(e),
            }
        }
        let mut action = None;
        egui::Panel::right("opponent-controls")
            .default_size(230.)
            .show(ui, |ui| {
                ui.heading(format!("KataGo · {}", self.settings.rank));
                ui.label(format!("You play {}", self.settings.player));
                if let Some(e) = &self.error {
                    ui.label(e);
                }
                if let Some(state) = &self.state {
                    ui.label(&state.status);
                    if let Some(result) = state.doc.nodes[0].property("RE") {
                        ui.label(format!("Result · {result}"));
                    }
                    if state.finished || self.error.is_some() {
                        if ui.button("Review game").clicked() {
                            action = Some(Action::Review(state.path.clone()));
                        }
                    } else {
                        if ui
                            .add_enabled(state.your_turn, egui::Button::new("Pass"))
                            .clicked()
                        {
                            let _ = client.input.send(Input::Play(state.doc.selected, None));
                        }
                        if ui.button("Resign").clicked() {
                            let _ = client.input.send(Input::Resign);
                        }
                    }
                    ui.small(format!("Saved in {}", state.path.display()));
                } else if self.error.is_none() {
                    ui.label("Starting KataGo…");
                }
                if ui.button("Back to review").clicked() {
                    action = Some(Action::Close);
                }
            });
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(s) = &self.state
                && let Some(point) = crate::ui::training_board(
                    ui,
                    &s.doc,
                    &s.board,
                    s.your_turn && self.error.is_none(),
                )
            {
                let _ = client.input.send(Input::Play(s.doc.selected, point));
            }
        });
        action
    }
}
