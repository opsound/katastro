use super::{
    ACCENT, Action, GROUP_DYING, GROUP_LIVE, GROUP_UNSETTLED, MOVE_LOSS_COLORS, MUTED,
    SUGGESTION_BLUE, caption, charts::chart, command, suggestion_color,
};
use crate::{
    Analysis, Color, Document,
    worker::{Command, Snapshot},
};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2, WidgetInfo, WidgetType};

pub(super) fn review_controls(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    group_strength: &mut bool,
    show_ai_moves: &mut bool,
    out: &mut Vec<Action>,
) {
    let mut muted = snapshot.sound_muted;
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(7.0, 5.0);
        ui.horizontal_wrapped(|ui| {
            for (visible, label, enabled, action) in [
                ("Open", "Open SGF", true, Action::Open),
                (
                    "Export",
                    "Export SGF",
                    snapshot.document.is_some(),
                    Action::Export,
                ),
                ("Games", "Games", true, Action::Games),
                ("Train", "Training", true, Action::Training),
                ("Engine", "Engine settings", true, Action::Settings),
            ] {
                let response = ui.add_enabled(enabled, egui::Button::new(visible));
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
                if response.on_hover_text(label).clicked() {
                    out.push(action);
                }
            }
            if snapshot.document.is_none() {
                super::mute_stones(ui, &mut muted);
            }
        });
    });
    if snapshot.document.is_none() {
        let preparing = snapshot.preparing_training;
        if ui
            .button(if preparing {
                "Pause library analysis"
            } else {
                "Analyze library"
            })
            .on_hover_text("Library analysis supplies comeback and blunder statistics and finds training opportunities.")
            .clicked()
        {
            command(
                out,
                if preparing {
                    Command::Pause
                } else {
                    Command::Start
                },
            );
        }
    }
    if let Some(source) = &snapshot.source {
        ui.add(
            egui::Label::new(caption(
                source.file_name().unwrap_or_default().to_string_lossy(),
            ))
            .truncate(),
        );
    }
    if let (Some(doc), Some(board)) = (&snapshot.document, &snapshot.board) {
        ui.columns(2, |columns| {
            player(
                &mut columns[0],
                Color::Black,
                doc.nodes[0].property("PB").unwrap_or("Black"),
            );
            player(
                &mut columns[1],
                Color::White,
                doc.nodes[0].property("PW").unwrap_or("White"),
            );
        });
        ui.horizontal(|ui| {
            ui.checkbox(show_ai_moves, "AI moves").on_hover_text("Show AI move suggestions on the board.");
            ui.checkbox(group_strength, "Group strength")
                .on_hover_text("Connected chains: green = likely to live, orange = unsettled, red = likely to die. Solid outlines indicate decisive expected ownership. This is KataGo's estimate under continued play, not a life-and-death proof; ko and seki can appear unsettled. Move-circle colors still show point loss.");
            super::mute_stones(ui, &mut muted);
        });
        // Keep controls fixed when ownership or a different move arrives.
        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 34.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_height(34.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            if *group_strength {
                if let Some(value) = snapshot.values.get(&doc.selected).filter(|a| a.ownership.len() == board.cells.len()) {
                    ui.horizontal(|ui| {
                        for (label, color) in [("Likely live", GROUP_LIVE), ("Unsettled", GROUP_UNSETTLED), ("Likely die", GROUP_DYING)] {
                            ui.label(RichText::new(label).color(color).size(11.0));
                        }
                    });
                    let depth = value.ownership_depth();
                    let label = if depth < crate::analysis::MIN_CACHE_VISITS {
                        format!("· quick estimate · {depth} {}", if depth == 1 { "visit" } else { "visits" })
                    } else { format!("· {depth} visits") };
                    ui.label(caption(label));
                } else {
                    ui.label(caption("Group estimates pending"))
                        .on_hover_text("Ownership appears as this position refines. Older cached charts remain usable while group estimates are pending.");
                }
            } else {
                ui.label(caption(format!("Move labels: point change for {}", board.next)));
            }
        });
    }
    if muted != snapshot.sound_muted {
        command(out, Command::SetSoundMuted(muted));
    }
}

pub(super) fn analysis_details(
    ui: &mut egui::Ui,
    doc: &Document,
    snapshot: &Snapshot,
    out: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading("Game analysis");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(if snapshot.running { "Pause" } else { "Analyze" })
                .clicked()
            {
                command(
                    out,
                    if snapshot.running {
                        Command::Pause
                    } else {
                        Command::Start
                    },
                );
            }
        });
    });
    ui.label(caption(match snapshot.analysis_target {
        Some(target) => format!(
            "{} {target} visits per position",
            if snapshot.running {
                "Refining:"
            } else {
                "Paused: target"
            }
        ),
        None => {
            if snapshot.running {
                "Preparing analysis".into()
            } else {
                "Analysis paused".into()
            }
        }
    }));
    ui.add_space(6.0);
    let selected = doc.selected;
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), 64.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            let value = snapshot.values.get(&selected);
            ui.columns(2, |columns| {
                columns[0].label(
                    RichText::new(
                        value
                            .map(|v| format!("{:+.1}", v.score_lead))
                            .unwrap_or("--".into()),
                    )
                    .size(26.0)
                    .color(ACCENT),
                );
                columns[0].label(caption("points · Black lead"));
                columns[1].label(
                    RichText::new(
                        value
                            .map(|v| format!("{:.1}%", v.winrate * 100.0))
                            .unwrap_or("--".into()),
                    )
                    .size(26.0),
                );
                columns[1].label(caption("Black winrate"));
            });
            ui.add_space(6.0);
            ui.label(caption(
                value
                    .map(|value| {
                        format!(
                            "{} visits · {}",
                            value.visits,
                            if value.visits < 64 {
                                "provisional"
                            } else {
                                "refined estimate"
                            }
                        )
                    })
                    .unwrap_or("Awaiting analysis".into()),
            ));
        },
    );
    ui.add_space(6.0);
    ui.add(
        egui::ProgressBar::new(if snapshot.coverage.1 == 0 {
            0.0
        } else {
            snapshot.coverage.0 as f32 / snapshot.coverage.1 as f32
        })
        .fill(ACCENT)
        .text(format!(
            "{} / {} played positions",
            snapshot.coverage.0, snapshot.coverage.1
        )),
    );
    ui.add_space(6.0);
    ui.label(caption("SCORE LEAD · POINTS").strong());
    chart(ui, snapshot, true, out);
    ui.add_space(8.0);
    ui.label(caption("WINRATE · BLACK %").strong());
    chart(ui, snapshot, false, out);
    ui.label(caption(
        "Click a curve to navigate · gold shows your variation.",
    ));
    ui.add_space(6.0);
    suggested_moves(
        ui,
        snapshot.values.get(&selected),
        doc.size,
        snapshot.board.as_ref().map_or(Color::Black, |b| b.next),
        out,
    );
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(8.0);
    if let Some(board) = &snapshot.board {
        ui.label(
            RichText::new(format!(
                "Move {} · {} to play",
                board.move_number, board.next
            ))
            .strong(),
        );
        ui.label(format!(
            "Captures  ● {}   ○ {}",
            board.captures[0], board.captures[1]
        ));
    }
    ui.label(format!(
        "{}×{} · {} rules · komi {}",
        doc.size, doc.size, doc.rules, doc.komi
    ));
    if doc.nodes[0].property("RU").is_none() {
        ui.label(
            RichText::new("SGF omits rules; Chinese rules assumed.")
                .color(Color32::from_rgb(221, 184, 108))
                .size(11.0),
        );
    }
    if !doc.mainline.contains(&doc.selected) {
        ui.colored_label(ACCENT, "Exploring a saved variation");
    }
    if let Some(comment) = doc.nodes[selected].property("C") {
        ui.add_space(10.0);
        ui.label(comment);
    }
    ui.add_space(6.0);
    navigation_legend(ui);
}

fn player(ui: &mut egui::Ui, color: Color, name: &str) {
    let response = ui
        .horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            ui.painter().circle_filled(
                rect.center(),
                7.0,
                if color == Color::Black {
                    Color32::from_rgb(25, 27, 30)
                } else {
                    Color32::from_rgb(248, 247, 242)
                },
            );
            ui.painter().circle_stroke(
                rect.center(),
                7.0,
                Stroke::new(1.0, Color32::from_rgb(128, 133, 139)),
            );
            ui.add(egui::Label::new(RichText::new(name).strong()).truncate())
                .on_hover_text(name);
        })
        .response;
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Other, true, format!("{} player: {name}", color))
    });
}

pub(super) fn navigation_legend(ui: &mut egui::Ui) {
    for vertical in [false, true] {
        ui.horizontal(|ui| {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(40.0, 18.0), Sense::hover());
            for (i, direction) in [-1.0, 1.0].iter().enumerate() {
                let center = rect.min + Vec2::new(10.0 + i as f32 * 20.0, 9.0);
                let vector = if vertical {
                    Vec2::new(0.0, *direction * 10.0)
                } else {
                    Vec2::new(*direction * 12.0, 0.0)
                };
                ui.painter()
                    .arrow(center - vector / 2.0, vector, Stroke::new(1.4, MUTED));
            }
            response.widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Other,
                    true,
                    if vertical {
                        "Up / Down variation arrows"
                    } else {
                        "Left / Right navigation arrows"
                    },
                )
            });
            ui.label(caption(if vertical {
                "Up / Down: variations"
            } else {
                "Left / Right: navigate moves"
            }));
        });
    }
    ui.label(caption(
        "P: pass   Space: pause
Cmd+O: open   Cmd+S: export",
    ));
}

fn suggested_moves(
    ui: &mut egui::Ui,
    analysis: Option<&Analysis>,
    size: usize,
    player: Color,
    out: &mut Vec<Action>,
) {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(7.0, 2.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), 147.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.label(caption("AI SUGGESTED MOVES").strong());
                suggestion_legend(ui);
                let count = analysis.map_or(0, |a| a.suggestions.len());
                let row_height = ui.spacing().interact_size.y;
                let list = egui::ScrollArea::vertical()
                    .id_salt("ai-candidates")
                    .max_height(100.0)
                    .min_scrolled_height(100.0)
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show_rows(ui, row_height, count.max(1), |ui, rows| {
                        for rank in rows {
                            suggestion_row(ui, analysis, size, player, rank, out);
                        }
                    });
                // Keep wheel gestures in the list when they reach its first or last row.
                if ui.rect_contains_pointer(list.inner_rect) {
                    ui.input_mut(|input| input.smooth_scroll_delta.y = 0.0);
                }
                ui.interact(
                    list.inner_rect,
                    ui.id().with("ai-candidate-list"),
                    Sense::hover(),
                )
                .widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "AI candidate list"));
            },
        );
    });
}

fn suggestion_legend(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Loss vs best:").size(10.0).color(MUTED));
        ui.colored_label(SUGGESTION_BLUE, RichText::new("best").size(10.0));
        for (loss, color) in MOVE_LOSS_COLORS {
            ui.colored_label(
                color,
                RichText::new(if loss == 6.0 {
                    "6+".into()
                } else {
                    loss.to_string()
                })
                .size(10.0),
            );
        }
        ui.colored_label(Color32::GRAY, RichText::new("?").size(10.0));
    })
    .response
    .on_hover_text(concat!(
        "Color compares against the best evaluated move: green within 0.5 points, ",
        "blending through yellow at 1.5, orange at 3, and red at 6 or more. ",
        "Gray means no score estimate. Alternatives below 25 visits are subdued; ",
        "visits measure search effort. Circle labels remain point changes from ",
        "the current position. Unscored previews stay in this list until point estimates arrive."
    ));
}

fn suggestion_row(
    ui: &mut egui::Ui,
    analysis: Option<&Analysis>,
    size: usize,
    player: Color,
    rank: usize,
    out: &mut Vec<Action>,
) {
    let suggested = analysis.and_then(|a| a.suggestions.get(rank));
    let color = analysis
        .filter(|_| suggested.is_some())
        .map_or(MUTED, |a| suggestion_color(a, rank, player));
    let label = suggested
        .map(|s| s.point.map(|p| p.gtp(size)).unwrap_or("Pass".into()))
        .unwrap_or("--".into());
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                suggested.is_some(),
                egui::Button::new(RichText::new(format!("{}  {label}", rank + 1)).color(color))
                    .min_size(Vec2::new(63.0, 18.0)),
            )
            .clicked()
            && let Some(s) = suggested
        {
            command(out, Command::Play(s.point));
        }
        let detail = suggested
            .map(|s| match (s.score_lead, s.winrate) {
                (Some(score), Some(winrate)) => format!(
                    "{score:+.1} pt   {:.1}% Black   {} visits",
                    winrate * 100.0,
                    s.visits
                ),
                _ => "Policy preview · no search yet".into(),
            })
            .unwrap_or_else(|| "Awaiting suggestions".into());
        ui.add(egui::Label::new(caption(detail)).truncate());
    });
}
