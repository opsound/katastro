use super::{Phase, Settings, Snapshot, client::Command};
use eframe::egui::{self, RichText};
use std::path::PathBuf;
mod charts;
pub struct View {
    pub open: bool,
    account: String,
    moves: usize,
    limit: usize,
    strict: bool,
    include_completed: bool,
}
impl Default for View {
    fn default() -> Self {
        Self {
            open: false,
            account: String::new(),
            moves: 5,
            limit: 20,
            strict: true,
            include_completed: false,
        }
    }
}
pub enum Action {
    Start {
        games: Vec<PathBuf>,
        account: String,
        settings: Settings,
    },
    Command(Command),
    Back,
    ExploreLine(Vec<Option<crate::Point>>),
}
impl View {
    pub fn show(&mut self, library: &crate::pandanet::Snapshot) {
        self.open = true;
        self.account = library.username.clone();
    }
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        state: &Snapshot,
        library: &crate::pandanet::Snapshot,
    ) -> Vec<Action> {
        let mut out = Vec::new();
        egui::Panel::top("training-heading").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Back to review").clicked() {
                    self.open = false;
                    out.push(Action::Back);
                }
                ui.heading("Hold the lead");
                if state.phase == Phase::Revealed {
                    ui.label(&state.status);
                    if ui.button("Exercise position").clicked() {
                        out.push(Action::ExploreLine(Vec::new()));
                    }
                    for (index, line) in state.lines.iter().enumerate() {
                        let text = line
                            .iter()
                            .map(|p| {
                                p.map(|p| p.gtp(state.document.as_ref().unwrap().size))
                                    .unwrap_or("pass".into())
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        if ui
                            .button(format!("Explore line {}", index + 1))
                            .on_hover_text(text)
                            .clicked()
                        {
                            out.push(Action::ExploreLine(line.clone()));
                        }
                    }
                    if ui.button("Next problem").clicked() {
                        out.push(Action::Command(Command::Next));
                    }
                }
            });
        });
        if state.phase == Phase::Revealed {
            return out;
        }
        if state.phase == Phase::Idle {
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .show(ui, |ui| self.setup(ui, state, library, &mut out));
            });
            return out;
        }
        egui::Panel::right("training-instructions")
            .resizable(true)
            .default_size(350.)
            .show(ui, |ui| {
                ui.add_space(16.);
                ui.heading(match state.phase {
                    Phase::Passed => "Passed",
                    Phase::Failed => "Not yet",
                    Phase::Searching => "Finding an exercise",
                    Phase::Revealing => "Preparing the review",
                    _ => "Keep your advantage",
                });
                ui.add_space(12.);
                ui.label(&state.status);
                if let Some(board) = &state.board {
                    ui.label(format!(
                        "You play {}",
                        if matches!(state.phase, Phase::YourTurn) {
                            if board.next == crate::Color::Black {
                                "Black"
                            } else {
                                "White"
                            }
                        } else {
                            // Only the turn itself needs a color label while playing.
                            "against 1 dan"
                        }
                    ));
                }
                if matches!(
                    state.phase,
                    Phase::YourTurn | Phase::Opponent | Phase::Grading
                ) {
                    ui.label(format!("{} / {} of your moves", state.played, state.target));
                    ui.label(
                        "Play on the board. Evaluations and hints stay hidden during the attempt.",
                    );
                    if ui
                        .add_enabled(state.phase == Phase::YourTurn, egui::Button::new("Pass"))
                        .clicked()
                    {
                        out.push(Action::Command(Command::Play(None)));
                    }
                }
                if let Some(report) = &state.report {
                    ui.add_space(16.);
                    ui.label("Total losses on your moves only:");
                    egui::Grid::new("training-loss-report").show(ui, |ui| {
                        ui.label("");
                        ui.label("Recorded");
                        ui.label("This attempt");
                        ui.end_row();
                        ui.label("Winrate lost");
                        ui.label(format!("{:.1} pp", report.recorded_loss.winrate * 100.0));
                        ui.label(format!("{:.1} pp", report.attempt_loss.winrate * 100.0));
                        ui.end_row();
                        ui.label("Score lost");
                        ui.label(format!("{:.1} points", report.recorded_loss.score));
                        ui.label(format!("{:.1} points", report.attempt_loss.score));
                        ui.end_row();
                    });
                    ui.label(format!(
                        "Reduction: {:+.1} pp · {:+.1} points",
                        report.winrate_saved() * 100.0,
                        report.score_saved()
                    ));
                    ui.label("Gains and opponent moves do not cancel your mistakes.");
                    if report.major_mistake {
                        ui.label(
                            "At least one of your moves lost 15 percentage points or 3 points.",
                        );
                    }
                    ui.add_space(16.);
                }
                if state.phase == Phase::Failed && ui.button("Retry without hints").clicked() {
                    out.push(Action::Command(Command::Retry));
                }
                if matches!(state.phase, Phase::Passed | Phase::Failed)
                    && ui.button("Next problem").clicked()
                {
                    out.push(Action::Command(Command::Next));
                }
                if state.phase == Phase::Passed {
                    if ui.button("Explore suggested lines").clicked() {
                        out.push(Action::Command(Command::Reveal));
                    }
                } else if matches!(
                    state.phase,
                    Phase::YourTurn | Phase::Opponent | Phase::Failed
                ) && ui.button("Give up and explore").clicked()
                {
                    out.push(Action::Command(Command::Reveal));
                }
                if matches!(
                    state.phase,
                    Phase::Searching | Phase::Grading | Phase::Revealing
                ) {
                    ui.spinner();
                }
                if let Some(error) = &state.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
            });
        if let Some(report) = &state.report {
            egui::Panel::bottom("training-report-comparison")
                .resizable(true)
                .default_size(200.0)
                .show(ui, |ui| charts::show(ui, &report.comparison));
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if let (Some(doc), Some(board)) = (&state.document, &state.board) {
                if let Some(point) =
                    crate::ui::training_board(ui, doc, board, state.phase == Phase::YourTurn)
                {
                    out.push(Action::Command(Command::Play(point)));
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Preparing a position from your game library…");
                });
            }
        });
        if state.phase == Phase::YourTurn
            && !ui.ctx().text_edit_focused()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::P))
        {
            out.push(Action::Command(Command::Play(None)));
        }
        out
    }
    fn setup(
        &mut self,
        ui: &mut egui::Ui,
        state: &Snapshot,
        library: &crate::pandanet::Snapshot,
        out: &mut Vec<Action>,
    ) {
        ui.add_space(35.0);
        ui.vertical_centered(|ui| {
            ui.heading("Find the move that keeps your advantage");
            ui.label("Practice turning points from your own games against a 1-dan opponent.");
        });
        ui.add_space(24.0);
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.scope(|ui| {
                ui.set_max_width(540.0);
                ui.label(format!("{} games available in your library", library.games.len()));
                ui.horizontal(|ui| {
                    ui.label("Your player name");
                    ui.add(egui::TextEdit::singleline(&mut self.account).hint_text("Name in your SGFs"))
                        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Training player name"));
                });
                ui.horizontal(|ui| {
                    ui.label("Search latest games");
                    ui.add(egui::DragValue::new(&mut self.limit).range(1..=library.games.len().max(1)));
                });
                ui.horizontal(|ui| {
                    ui.label("Your moves per attempt");
                    for n in [5, 10] { ui.selectable_value(&mut self.moves, n, n.to_string()); }
                });
                ui.checkbox(&mut self.strict, "Require no further major mistake");
                ui.checkbox(&mut self.include_completed, "Include completed exercises");
                ui.add_space(12.0);
                ui.label("Exercises start where you had at least a 65% winrate and a positive score, then lost at least 15 percentage points on your move.");
                ui.label("Pass by reducing the sum of losses on your moves by 10 percentage points of winrate or 3 points of score compared with your recorded moves, without materially worsening the other measure. Gains and opponent moves never cancel your losses. A major mistake loses 15 percentage points or 3 points on one move.");
                ui.label("Hints and the recorded continuation stay hidden. After a failed attempt, you can retry without seeing a solution. Pass or give up to explore up to three suggested lines.");
                ui.add_space(12.0);
                ui.label(RichText::new("Opponent: 1 dan · HumanSL\nApproximate human rank. The 99 MB model downloads once, if needed. Games stay on this Mac.").small().weak());
                ui.add_space(12.0);
                if ui.add_enabled(!library.games.is_empty() && !self.account.trim().is_empty(), egui::Button::new("Start training")).clicked() { out.push(self.start(library)); }
                if library.games.is_empty() { ui.label("Sync some games from Pandanet to begin."); }
                if !state.status.is_empty() { ui.label(&state.status); }
                if let Some(error) = &state.error { ui.colored_label(egui::Color32::LIGHT_RED, error); }
            });
        });
    }
    fn start(&self, library: &crate::pandanet::Snapshot) -> Action {
        let mut games = library.games.iter().collect::<Vec<_>>();
        games.sort_by(|a, b| (&b.date, &b.id).cmp(&(&a.date, &a.id)));
        Action::Start {
            games: games
                .into_iter()
                .take(self.limit)
                .map(|g| g.path.clone())
                .collect(),
            account: self.account.clone(),
            settings: Settings {
                moves: self.moves,
                strict: self.strict,
                include_completed: self.include_completed,
            },
        }
    }
}
