use crate::{
    Analysis, Color, Document, Point,
    worker::{Command, Snapshot},
};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2, WidgetInfo,
    WidgetType,
};
use egui_plot::{Line, Plot, PlotUi, Points};
#[derive(Clone, Debug)]
pub enum Action {
    Review(Command),
    Open,
    Export,
    Settings,
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
const CHART_VARIATION: Color32 = Color32::from_rgb(255, 195, 105);
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
    egui::Panel::top("header")
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(26, 30, 36))
                .inner_margin(16),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (logo, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                ui.painter()
                    .circle_stroke(logo.center(), 9.0, Stroke::new(1.5, ACCENT));
                ui.painter().circle_filled(logo.center(), 3.5, ACCENT);
                response.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Other, true, "Katastro emblem")
                });
                ui.label(RichText::new("KATASTRO").strong().size(19.0));
                ui.add_space(14.0);
                if let Some(source) = &snapshot.source {
                    ui.label(
                        RichText::new(source.file_name().unwrap_or_default().to_string_lossy())
                            .color(MUTED),
                    );
                } else {
                    ui.label(RichText::new("Go game review").color(MUTED));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Engine settings").clicked() {
                        out.push(Action::Settings);
                    }
                    if ui
                        .add_enabled(snapshot.document.is_some(), egui::Button::new("Export SGF"))
                        .clicked()
                    {
                        out.push(Action::Export);
                    }
                    if ui.button("Open SGF").clicked() {
                        out.push(Action::Open);
                    }
                });
            });
        });
    egui::Panel::bottom("status")
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(26, 30, 36))
                .inner_margin(9),
        )
        .show(ui, |ui| {
            if let Some(error) = &snapshot.error {
                ui.colored_label(Color32::from_rgb(255, 151, 136), error);
            } else {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        if snapshot.running { ACCENT } else { MUTED },
                        if snapshot.running { "●" } else { "○" },
                    );
                    ui.label(&snapshot.status);
                });
            }
        });
    if let Some(doc) = &snapshot.document {
        egui::Panel::bottom("tree")
            .resizable(true)
            .default_size(156.0)
            .min_size(100.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("VARIATIONS").color(MUTED).size(11.0).strong());
                    ui.label(
                        RichText::new(
                            "Played game stays on the top row · click any node to explore",
                        )
                        .color(MUTED)
                        .size(11.0),
                    );
                });
                tree(ui, doc, &snapshot.source, &mut out);
            });
        egui::Panel::right("analysis")
            .resizable(true)
            .default_size(365.0)
            .size_range(300.0..=520.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(26, 30, 36))
                    .inner_margin(20),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading("Game analysis");
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button(if snapshot.running { "Pause" } else { "Analyze" })
                                        .clicked()
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
                                },
                            );
                        });
                        ui.label(RichText::new("Black's perspective").color(MUTED).size(12.0));
                        ui.label(
                            RichText::new(match snapshot.analysis_target {
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
                            })
                            .size(11.0)
                            .color(MUTED),
                        );
                        ui.add_space(12.0);
                        let selected = doc.selected;
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), 96.0),
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
                                        .size(31.0)
                                        .color(ACCENT),
                                    );
                                    columns[0].label(
                                        RichText::new("points · Black lead")
                                            .color(MUTED)
                                            .size(11.0),
                                    );
                                    columns[1].label(
                                        RichText::new(
                                            value
                                                .map(|v| format!("{:.1}%", v.winrate * 100.0))
                                                .unwrap_or("--".into()),
                                        )
                                        .size(31.0),
                                    );
                                    columns[1].label(
                                        RichText::new("Black winrate").color(MUTED).size(11.0),
                                    );
                                });
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
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
                                    )
                                    .color(MUTED)
                                    .size(11.0),
                                );
                            },
                        );
                        ui.add_space(12.0);
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
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new("SCORE LEAD · POINTS")
                                .size(11.0)
                                .color(MUTED)
                                .strong(),
                        );
                        chart(ui, snapshot, true, &mut out);
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("WINRATE · BLACK %")
                                .size(11.0)
                                .color(MUTED)
                                .strong(),
                        );
                        chart(ui, snapshot, false, &mut out);
                        ui.label(
                            RichText::new("Click a curve to navigate · gold shows your variation.")
                                .size(11.0)
                                .color(MUTED),
                        );
                        ui.add_space(12.0);
                        suggested_moves(
                            ui,
                            snapshot.values.get(&selected),
                            doc.size,
                            snapshot.board.as_ref().map_or(Color::Black, |b| b.next),
                            &mut out,
                        );
                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(8.0);
                        if let Some(board) = &snapshot.board {
                            ui.label(
                                RichText::new(format!(
                                    "Move {} · {} to play",
                                    board.move_number,
                                    if board.next == Color::Black {
                                        "Black"
                                    } else {
                                        "White"
                                    }
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
                        ui.add_space(15.0);
                        navigation_legend(ui);
                    });
            });
    }
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(20, 23, 28))
                .inner_margin(18),
        )
        .show(ui, |ui| {
            if let (Some(doc), Some(board)) = (&snapshot.document, &snapshot.board) {
                ui.horizontal(|ui| {
                    player(
                        ui,
                        Color::Black,
                        doc.nodes[0].property("PB").unwrap_or("Black"),
                    );
                    ui.label(RichText::new("vs").color(MUTED));
                    player(
                        ui,
                        Color::White,
                        doc.nodes[0].property("PW").unwrap_or("White"),
                    );
                });
                ui.label(
                    RichText::new(format!(
                        "Move labels: point change for {}",
                        if board.next == Color::Black {
                            "Black"
                        } else {
                            "White"
                        }
                    ))
                    .size(11.0)
                    .color(MUTED),
                );
                ui.add_space(9.0);
                board_view(ui, doc, board, snapshot, &mut out);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    for (label, value) in [
                        ("First move", Command::First),
                        ("Previous move", Command::Step(false)),
                        ("Next move", Command::Step(true)),
                        ("Last move", Command::Last),
                        ("Pass", Command::Play(None)),
                    ] {
                        if ui.button(label).clicked() {
                            command(&mut out, value);
                        }
                    }
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
fn board_view(
    ui: &mut egui::Ui,
    doc: &Document,
    board: &crate::Board,
    snapshot: &Snapshot,
    out: &mut Vec<Action>,
) {
    let analysis = snapshot.values.get(&doc.selected);
    let recorded = next_recorded_move(doc);
    // Prefer the move's estimate from this same search over an older child evaluation.
    let recorded_score = recorded.and_then(|(id, played)| {
        analysis
            .and_then(|a| a.suggestions.iter().find(|s| s.point == played.point))
            .and_then(|s| s.score_lead)
            .or_else(|| snapshot.values.get(&id).map(|a| a.score_lead))
    });
    let side = ui
        .available_width()
        .min((ui.available_height() - 56.0).max(150.0))
        .max(140.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 12, Color32::from_rgb(221, 185, 130));
    let padding = (side * 0.055).max(20.0);
    let start = rect.min + Vec2::splat(padding);
    let extent = side - 2.0 * padding;
    let gap = extent / (doc.size - 1) as f32;
    let marker_radius = gap * 0.43;
    let grid = Stroke::new(1.0, Color32::from_rgb(108, 81, 48));
    for i in 0..doc.size {
        let offset = i as f32 * gap;
        painter.line_segment(
            [
                start + Vec2::new(offset, 0.0),
                start + Vec2::new(offset, extent),
            ],
            grid,
        );
        painter.line_segment(
            [
                start + Vec2::new(0.0, offset),
                start + Vec2::new(extent, offset),
            ],
            grid,
        );
        let letter = b"ABCDEFGHJKLMNOPQRST"[i] as char;
        for y in [rect.top() + padding * 0.35, rect.bottom() - padding * 0.35] {
            painter.text(
                Pos2::new(start.x + offset, y),
                Align2::CENTER_CENTER,
                letter,
                FontId::proportional(11.0),
                Color32::from_rgb(93, 70, 45),
            );
        }
        for x in [rect.left() + padding * 0.35, rect.right() - padding * 0.35] {
            painter.text(
                Pos2::new(x, start.y + offset),
                Align2::CENTER_CENTER,
                (doc.size - i).to_string(),
                FontId::proportional(11.0),
                Color32::from_rgb(93, 70, 45),
            );
        }
    }
    let stars = match doc.size {
        19 => vec![3, 9, 15],
        13 => vec![3, 6, 9],
        9 => vec![2, 4, 6],
        _ => Vec::new(),
    };
    for x in &stars {
        for y in &stars {
            if doc.size == 19 || x == y || (*x != doc.size / 2 && *y != doc.size / 2) {
                painter.circle_filled(
                    start + Vec2::new(*x as f32 * gap, *y as f32 * gap),
                    2.7,
                    grid.color,
                );
            }
        }
    }
    for y in 0..doc.size {
        for x in 0..doc.size {
            let point = Point::new(x, y);
            let center = start + Vec2::new(x as f32 * gap, y as f32 * gap);
            let stone = board.stone(point);
            let response = ui.interact(
                Rect::from_center_size(center, Vec2::splat(gap * 0.92)),
                ui.id().with(("intersection", x, y)),
                if stone.is_none() {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            response.widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Button,
                    stone.is_none(),
                    format!("Play {}", point.gtp(doc.size)),
                )
            });
            if let Some(color) = stone {
                painter.circle_filled(
                    center + Vec2::new(1.4, 2.0),
                    gap * 0.455,
                    Color32::from_black_alpha(38),
                );
                painter.circle_filled(
                    center,
                    gap * 0.455,
                    if color == Color::Black {
                        Color32::from_rgb(30, 32, 35)
                    } else {
                        Color32::from_rgb(248, 247, 242)
                    },
                );
                painter.circle_stroke(
                    center,
                    gap * 0.455,
                    Stroke::new(
                        0.8,
                        if color == Color::Black {
                            Color32::BLACK
                        } else {
                            Color32::from_rgb(178, 175, 164)
                        },
                    ),
                );
            } else if response.hovered() {
                painter.circle_filled(
                    center,
                    gap * 0.445,
                    if board.next == Color::Black {
                        Color32::from_black_alpha(100)
                    } else {
                        Color32::from_white_alpha(145)
                    },
                );
            }
            if response.clicked() {
                command(out, Command::Play(Some(point)));
            }
        }
    }
    if let Some(analysis) = analysis {
        for (rank, suggestion) in analysis.suggestions.iter().enumerate() {
            let Some(point) = suggestion.point.filter(|_| suggestion.score_lead.is_some()) else {
                continue;
            };
            if point.x >= doc.size || point.y >= doc.size || board.stone(point).is_some() {
                continue;
            }
            let center = start + Vec2::new(point.x as f32 * gap, point.y as f32 * gap);
            let color = suggestion_color(analysis, rank, board.next);
            painter.circle_filled(center, marker_radius, color);
            paint_move_delta(
                painter,
                center,
                marker_radius,
                point_delta(Some(analysis), suggestion.score_lead, board.next),
                if color.a() < 255
                    || u32::from(color.r()) * 299
                        + u32::from(color.g()) * 587
                        + u32::from(color.b()) * 114
                        > 150_000
                {
                    Color32::from_rgb(44, 39, 32)
                } else {
                    Color32::WHITE
                },
            );
            ui.interact(
                Rect::from_center_size(center, Vec2::splat(2.0 * marker_radius + 1.0)),
                ui.id().with(("suggestion", rank)),
                Sense::hover(),
            )
            .widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Other,
                    true,
                    format!("AI suggestion {}: {}", rank + 1, point.gtp(doc.size)),
                )
            });
        }
    }
    if let Some((_, played)) = recorded
        && let Some(point) = played.point.filter(|p| board.stone(*p).is_none())
    {
        let center = start + Vec2::new(point.x as f32 * gap, point.y as f32 * gap);
        let color = if played.color == Color::Black {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        for i in 0..24 {
            let points = (0..=3)
                .map(|j| {
                    let angle = std::f32::consts::TAU * (i as f32 + j as f32 / 6.0) / 24.0;
                    center + Vec2::angled(angle) * marker_radius
                })
                .collect();
            painter.add(egui::Shape::line(points, Stroke::new(2.1, color)));
        }
        let already_labeled = analysis.is_some_and(|a| {
            a.suggestions
                .iter()
                .any(|s| s.point == Some(point) && s.score_lead.is_some())
        });
        if !already_labeled {
            paint_move_delta(
                painter,
                center,
                marker_radius,
                point_delta(analysis, recorded_score, played.color),
                Color32::from_rgb(44, 39, 32),
            );
        }
        ui.interact(
            Rect::from_center_size(center, Vec2::splat(2.0 * marker_radius + 1.0)),
            ui.id().with("next-recorded"),
            Sense::hover(),
        )
        .widget_info(|| {
            WidgetInfo::labeled(
                WidgetType::Other,
                true,
                format!(
                    "Next recorded move: {} {}",
                    if played.color == Color::Black {
                        "Black"
                    } else {
                        "White"
                    },
                    point.gtp(doc.size)
                ),
            )
        });
    }
    if let Some(point) = doc.nodes[doc.selected].played.and_then(|m| m.point) {
        let center = start + Vec2::new(point.x as f32 * gap, point.y as f32 * gap);
        painter.circle_stroke(
            center,
            gap * 0.17,
            Stroke::new(
                2.0,
                if board.stone(point) == Some(Color::Black) {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(44, 50, 50)
                },
            ),
        );
    }
}
fn tree(
    ui: &mut egui::Ui,
    doc: &Document,
    source: &Option<std::path::PathBuf>,
    out: &mut Vec<Action>,
) {
    let layout = doc.layout();
    let max_col = layout.iter().map(|p| p.column).max().unwrap_or(0);
    let max_row = layout.iter().map(|p| p.row).max().unwrap_or(0);
    let focus = (source.clone(), doc.selected);
    let id = ui.id().with("tree-selection");
    let follow = ui.data_mut(|data| {
        let changed = data
            .get_temp::<(Option<std::path::PathBuf>, usize)>(id)
            .as_ref()
            != Some(&focus);
        data.insert_temp(id, focus);
        changed
    });
    let output = egui::ScrollArea::both()
        .id_salt("variation-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(
                    ((max_col + 1) as f32 * 44.0 + 20.0).max(ui.available_width()),
                    (max_row + 1) as f32 * 40.0 + 10.0,
                ),
                Sense::hover(),
            );
            let position = |id: usize| {
                rect.min
                    + Vec2::new(
                        26.0 + layout[id].column as f32 * 44.0,
                        24.0 + layout[id].row as f32 * 40.0,
                    )
            };
            if follow {
                ui.scroll_to_rect_animation(
                    Rect::from_center_size(position(doc.selected), Vec2::splat(40.0)),
                    None,
                    egui::style::ScrollAnimation::none(),
                );
            }
            let painter = ui.painter();
            for p in &layout {
                if let Some(parent) = doc.nodes[p.id].parent {
                    let from = position(parent);
                    let to = position(p.id);
                    painter.line_segment(
                        [from, Pos2::new(from.x, to.y)],
                        Stroke::new(1.5, Color32::from_rgb(67, 76, 85)),
                    );
                    painter.line_segment(
                        [Pos2::new(from.x, to.y), to],
                        Stroke::new(1.5, Color32::from_rgb(67, 76, 85)),
                    );
                }
            }
            for p in &layout {
                let center = position(p.id);
                let hit = Rect::from_center_size(center, Vec2::splat(32.0));
                if !ui.is_rect_visible(hit) {
                    continue;
                }
                let response = ui.interact(hit, ui.id().with(("node", p.id)), Sense::click());
                response.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, format!("Tree node {}", p.id))
                });
                if doc.selected == p.id {
                    painter.rect_filled(hit.expand(2.0), 7, ACCENT);
                }
                let color = doc.nodes[p.id].played.map(|m| m.color);
                let fill = match color {
                    Some(Color::Black) => Color32::from_rgb(30, 32, 35),
                    Some(Color::White) => Color32::from_rgb(235, 237, 234),
                    None => Color32::from_rgb(67, 76, 85),
                };
                painter.circle_filled(center, 13.0, fill);
                painter.circle_stroke(
                    center,
                    13.0,
                    Stroke::new(1.0, Color32::from_rgb(114, 125, 134)),
                );
                painter.text(
                    center,
                    Align2::CENTER_CENTER,
                    if p.id == 0 {
                        "•".into()
                    } else {
                        doc.path(p.id)
                            .map(|path| {
                                path.iter()
                                    .filter(|id| doc.nodes[**id].played.is_some())
                                    .count()
                                    .to_string()
                            })
                            .unwrap_or_default()
                    },
                    FontId::proportional(10.0),
                    if color == Some(Color::White) {
                        Color32::from_rgb(30, 32, 35)
                    } else {
                        Color32::WHITE
                    },
                );
                if response.clicked() {
                    command(out, Command::Select(p.id));
                }
                response.on_hover_text(if doc.mainline.contains(&p.id) {
                    "Played game"
                } else {
                    "Saved variation"
                });
            }
        });
    ui.interact(
        output.inner_rect,
        ui.id().with("timeline-viewport"),
        Sense::hover(),
    )
    .widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Timeline viewport"));
}
pub fn chart_points(snapshot: &Snapshot, score: bool) -> Vec<Option<[f64; 2]>> {
    snapshot
        .document
        .as_ref()
        .map(|doc| {
            doc.mainline
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    snapshot.values.get(id).map(|a| {
                        [
                            i as f64,
                            if score {
                                a.score_lead
                            } else {
                                a.winrate * 100.0
                            },
                        ]
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn assessment(snapshot: &Snapshot, node: usize, score: bool) -> Option<f64> {
    snapshot.values.get(&node).map(|value| {
        if score {
            value.score_lead
        } else {
            value.winrate * 100.0
        }
    })
}

fn paint_chart_series(
    plot: &mut PlotUi<'_>,
    name: &str,
    points: &[Option<[f64; 2]>],
    color: Color32,
    width: f32,
) {
    let mut segment = Vec::new();
    for point in points.iter().copied().chain(std::iter::once(None)) {
        if let Some(point) = point {
            segment.push(point);
        } else {
            if segment.len() >= 2 {
                plot.line(
                    Line::new(name, std::mem::take(&mut segment))
                        .color(color)
                        .width(width),
                );
            }
            segment.clear();
        }
    }
    plot.points(
        Points::new(name, points.iter().flatten().copied().collect::<Vec<_>>())
            .color(color)
            .radius(1.4),
    );
}

fn chart(ui: &mut egui::Ui, snapshot: &Snapshot, score: bool, out: &mut Vec<Action>) {
    let Some(doc) = &snapshot.document else {
        return;
    };
    let all = chart_points(snapshot, score);
    let mut path = doc.mainline.clone();
    let fork = if doc.mainline.contains(&doc.selected) {
        None
    } else {
        path = doc.path(doc.selected).unwrap_or_default();
        while let Some(child) = path.last().and_then(|id| doc.nodes[*id].children.first()) {
            path.push(*child);
        }
        Some(
            path.iter()
                .zip(&doc.mainline)
                .take_while(|(a, b)| a == b)
                .count()
                .saturating_sub(1),
        )
    };
    let variation: Vec<_> = path
        .iter()
        .enumerate()
        .map(|(index, id)| assessment(snapshot, *id, score).map(|value| [index as f64, value]))
        .collect();
    let game_color = if score {
        ACCENT
    } else {
        Color32::from_rgb(126, 167, 244)
    };
    let mut plot = Plot::new(if score { "score" } else { "winrate" })
        .height(145.0)
        .include_x(0.0)
        .include_x(doc.mainline.len().max(path.len()).saturating_sub(1).max(1) as f64)
        .include_y(if score { 0.0 } else { 50.0 })
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false);
    if !score {
        plot = plot.include_y(0.0).include_y(100.0);
    } else {
        plot = plot.include_y(-5.0).include_y(5.0);
    }
    let response = plot.show(ui, |plot_ui| {
        if let Some(fork) = fork {
            paint_chart_series(
                plot_ui,
                "Played continuation",
                &all[fork..],
                game_color.gamma_multiply(0.3),
                1.5,
            );
            paint_chart_series(plot_ui, "Shared game", &all[..=fork], game_color, 2.0);
            paint_chart_series(
                plot_ui,
                "Variation",
                &variation[fork..],
                CHART_VARIATION,
                2.5,
            );
        } else {
            paint_chart_series(plot_ui, "Played game", &all, game_color, 2.0);
        }
        if let Some(index) = path.iter().position(|id| *id == doc.selected)
            && let Some(value) = assessment(snapshot, doc.selected, score)
        {
            plot_ui.points(
                Points::new("Selected", vec![[index as f64, value]])
                    .radius(4.0)
                    .color(Color32::WHITE),
            );
        }
        plot_ui.pointer_coordinate()
    });
    response.response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Other,
            true,
            if score {
                "Score lead chart"
            } else {
                "Winrate chart"
            },
        )
    });
    if response.response.clicked()
        && let Some(point) = response.inner
    {
        let index = (point.x.round().max(0.0) as usize).min(doc.mainline.len().max(path.len()) - 1);
        let distance = |node| {
            assessment(snapshot, node, score)
                .map(|value| (value - point.y).abs())
                .unwrap_or(f64::INFINITY)
        };
        let node = match (path.get(index), doc.mainline.get(index)) {
            (Some(active), Some(played)) if distance(*played) < distance(*active) => *played,
            (Some(active), _) => *active,
            (_, Some(played)) => *played,
            _ => return,
        };
        command(out, Command::Select(node));
    }
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
            ui.label(RichText::new(name).strong());
        })
        .response;
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Other,
            true,
            format!(
                "{} player: {name}",
                if color == Color::Black {
                    "Black"
                } else {
                    "White"
                }
            ),
        )
    });
}
fn navigation_legend(ui: &mut egui::Ui) {
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
            ui.label(
                RichText::new(if vertical {
                    "Up / Down: variations"
                } else {
                    "Left / Right: navigate moves"
                })
                .size(11.0)
                .color(MUTED),
            );
        });
    }
    ui.label(
        RichText::new(
            "P: pass   Space: pause
Cmd+O: open   Cmd+S: export",
        )
        .size(11.0)
        .color(MUTED),
    );
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
                ui.label(
                    RichText::new("AI SUGGESTED MOVES")
                        .size(11.0)
                        .strong()
                        .color(MUTED),
                );
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
        ui.add(egui::Label::new(RichText::new(detail).size(11.0).color(MUTED)).truncate());
    });
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

fn next_recorded_move(doc: &Document) -> Option<(crate::NodeId, crate::Move)> {
    let mut next = doc.nodes[doc.selected].children.first().copied();
    while let Some(id) = next {
        if let Some(played) = doc.nodes[id].played {
            return Some((id, played));
        }
        next = doc.nodes[id].children.first().copied();
    }
    None
}
fn point_delta(
    current: Option<&Analysis>,
    resulting_score: Option<f64>,
    color: Color,
) -> Option<f64> {
    let delta =
        (resulting_score? - current?.score_lead) * if color == Color::Black { 1.0 } else { -1.0 };
    Some(if delta.abs() < 0.05 { 0.0 } else { delta })
}
fn paint_move_delta(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    delta: Option<f64>,
    color: Color32,
) {
    let label = delta.map(|v| format!("{v:+.1}")).unwrap_or("--".into());
    let mut size = (radius * 0.8).clamp(7.0, 16.0);
    let width = painter
        .layout_no_wrap(label.clone(), FontId::proportional(size), color)
        .size()
        .x;
    if width > radius * 1.7 {
        size *= radius * 1.7 / width;
    }
    painter.text(
        center,
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(size),
        color,
    );
}
