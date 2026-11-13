use crate::{
    Analysis, Color,
    worker::{Command, Snapshot},
};
use eframe::egui::{self, Color32, RichText, Vec2};
mod board;
mod charts;
mod sidebar;
mod tree;
use board::board_view;
pub use charts::chart_points;
use sidebar::{analysis_details, navigation_legend, review_controls};
use tree::{move_buttons, tree};
#[derive(Clone, Debug)]
pub enum Action {
    Review(Command),
    Open,
    Export,
    Settings,
    Pandanet,
    Games,
    Training,
}
const ACCENT: Color32 = Color32::from_rgb(95, 206, 175);
const SUGGESTION_BLUE: Color32 = Color32::from_rgb(65, 145, 255);
const MOVE_LOSS_COLORS: [(f64, Color32); 4] = [
    (0.5, Color32::from_rgb(49, 121, 86)),
    (1.5, Color32::from_rgb(219, 194, 71)),
    (3.0, Color32::from_rgb(231, 139, 56)),
    (6.0, Color32::from_rgb(208, 66, 62)),
];
const MUTED: Color32 = Color32::from_rgb(143, 153, 164);
const GROUP_LIVE: Color32 = Color32::from_rgb(40, 150, 88);
const GROUP_UNSETTLED: Color32 = Color32::from_rgb(230, 150, 45);
const GROUP_DYING: Color32 = Color32::from_rgb(210, 63, 63);
fn caption(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED).size(11.0)
}
fn command(out: &mut Vec<Action>, value: Command) {
    out.push(Action::Review(value));
}

pub fn render(ui: &mut egui::Ui, snapshot: &Snapshot) -> Vec<Action> {
    let mut out = Vec::new();
    if !ui.ctx().text_edit_focused() {
        ui.input_mut(|input| {
            for (key, down) in [(egui::Key::ArrowUp, false), (egui::Key::ArrowDown, true)] {
                if input.consume_key(egui::Modifiers::NONE, key)
                    && let Some(node) = snapshot
                        .document
                        .as_ref()
                        .and_then(|doc| doc.vertical_neighbor(down))
                {
                    command(&mut out, Command::Select(node));
                }
            }
            for (key, value) in [
                (egui::Key::ArrowLeft, Command::Step(false)),
                (egui::Key::ArrowRight, Command::Step(true)),
                (egui::Key::Home, Command::First),
                (egui::Key::End, Command::Last),
                (egui::Key::P, Command::Play(None)),
            ] {
                if snapshot.document.is_some() && input.consume_key(egui::Modifiers::NONE, key) {
                    command(&mut out, value);
                }
            }
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::O) {
                out.push(Action::Open);
            }
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::S) {
                out.push(Action::Export);
            }
            if snapshot.document.is_some()
                && input.consume_key(egui::Modifiers::NONE, egui::Key::Space)
            {
                command(
                    &mut out,
                    if snapshot.running {
                        Command::Pause
                    } else {
                        Command::Start
                    },
                );
            }
        });
    }
    let strength_id = ui.id().with("group-strength");
    let suggestions_id = ui.id().with("board-ai-moves");
    let (mut group_strength, mut show_ai_moves) = ui.ctx().data_mut(|data| {
        (
            data.get_temp::<bool>(strength_id).unwrap_or(false),
            data.get_temp::<bool>(suggestions_id).unwrap_or(true),
        )
    });
    egui::Panel::right("analysis")
        .resizable(true)
        .default_size(365.0)
        .size_range(300.0..=520.0)
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(26, 30, 36))
                .inner_margin(14),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            egui::Panel::bottom("review-status")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 9)))
                .show(ui, |ui| {
                    let (text, color) = if let Some(error) = &snapshot.error {
                        (error, Color32::from_rgb(255, 151, 136))
                    } else {
                        (
                            &snapshot.status,
                            if snapshot.running { ACCENT } else { MUTED },
                        )
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Pandanet").clicked() {
                            out.push(Action::Pandanet);
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(text).color(color)).truncate())
                                .on_hover_text(text);
                        });
                    });
                });
            if let Some(doc) = &snapshot.document {
                egui::Panel::bottom("review-tree")
                    .resizable(true)
                    .default_size(200.0)
                    .size_range(160.0..=320.0)
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        move_buttons(ui, &mut out);
                        ui.horizontal(|ui| {
                            ui.label(caption("VARIATIONS").strong());
                            ui.label(caption("Played game on top"));
                        });
                        tree(ui, doc, &snapshot.source, &mut out);
                    });
            }
            egui::Panel::top("review-controls")
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    review_controls(
                        ui,
                        snapshot,
                        &mut group_strength,
                        &mut show_ai_moves,
                        &mut out,
                    );
                });
            egui::ScrollArea::vertical()
                .id_salt("analysis-details")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                    if let Some(doc) = &snapshot.document {
                        analysis_details(ui, doc, snapshot, &mut out);
                    } else {
                        ui.label(
                            RichText::new("Open a game to review its analysis and variations.")
                                .color(MUTED),
                        );
                        navigation_legend(ui);
                    }
                });
        });
    ui.ctx().data_mut(|data| {
        data.insert_temp(strength_id, group_strength);
        data.insert_temp(suggestions_id, show_ai_moves);
    });
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(20, 23, 28))
                .inner_margin(8),
        )
        .show(ui, |ui| {
            if let (Some(doc), Some(board)) = (&snapshot.document, &snapshot.board) {
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.add_space(((ui.available_height() - ui.available_width()) / 2.0).max(0.0));
                    board_view(
                        ui,
                        doc,
                        board,
                        snapshot,
                        group_strength,
                        show_ai_moves,
                        &mut out,
                    );
                });
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.25).max(20.0));
                    ui.label(RichText::new("Every move tells a story.").size(31.0));
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(
                            "Open a game, explore its variations, and see where it changed.",
                        )
                        .color(MUTED),
                    );
                    ui.add_space(24.0);
                    if ui
                        .add(egui::Button::new("Open SGF").min_size(Vec2::new(170.0, 42.0)))
                        .clicked()
                    {
                        out.push(Action::Open);
                    }
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new("You can also drop an SGF into this window.")
                            .color(MUTED)
                            .size(12.0),
                    );
                });
            }
        });
    out
}

fn suggestion_color(analysis: &Analysis, rank: usize, player: Color) -> Color32 {
    let suggestion = &analysis.suggestions[rank];
    let color = match (
        suggestion.score_lead,
        analysis.suggestions.first().and_then(|s| s.score_lead),
    ) {
        (Some(_), Some(_)) if rank == 0 => SUGGESTION_BLUE,
        (Some(score), Some(best)) => {
            let loss = ((best - score) * if player == Color::Black { 1.0 } else { -1.0 }).max(0.0);
            let stops = MOVE_LOSS_COLORS;
            if loss <= stops[0].0 {
                stops[0].1
            } else {
                stops
                    .windows(2)
                    .find(|pair| loss <= pair[1].0)
                    .map_or(stops[3].1, |pair| {
                        pair[0].1.lerp_to_gamma(
                            pair[1].1,
                            ((loss - pair[0].0) / (pair[1].0 - pair[0].0)) as f32,
                        )
                    })
            }
        }
        _ => Color32::GRAY,
    };
    if rank > 0 && suggestion.visits < 25 {
        color.gamma_multiply(0.45)
    } else {
        color
    }
}

/// Training supplies only the played prefix, with no evaluations or future nodes.
pub(crate) fn training_board(
    ui: &mut egui::Ui,
    doc: &crate::Document,
    board: &crate::Board,
    enabled: bool,
) -> Option<Option<crate::Point>> {
    let empty = Snapshot::empty(crate::engine::EngineConfig {
        executable: Default::default(),
        model: Default::default(),
    });
    let mut actions = Vec::new();
    let opacity = ui.painter().opacity();
    ui.add_enabled_ui(enabled, |ui| {
        // Block interaction while thinking, without fading the position being studied.
        ui.set_opacity(opacity);
        board_view(ui, doc, board, &empty, false, false, &mut actions)
    });
    actions.into_iter().find_map(|a| {
        if let Action::Review(Command::Play(p)) = a {
            Some(p)
        } else {
            None
        }
    })
}
