use crate::{
    Analysis, Color, Document, Point,
    worker::{Command, Snapshot},
};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2, WidgetInfo,
    WidgetType,
};
use egui_plot::{Line, Plot, Points};
#[derive(Clone, Debug)]
pub enum Action {
    Review(Command),
    Open,
    Export,
    Settings,
}
const ACCENT: Color32 = Color32::from_rgb(95, 206, 175);
const MUTED: Color32 = Color32::from_rgb(143, 153, 164);
fn command(out: &mut Vec<Action>, value: Command) {
    out.push(Action::Review(value));
}
pub fn render(ui: &mut egui::Ui, snapshot: &Snapshot) -> Vec<Action> {
    let mut out = Vec::new();
    if !ui.ctx().egui_wants_keyboard_input() {
        ui.input_mut(|input| {
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
                ui.label(RichText::new("◉").size(24.0).color(ACCENT));
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
                tree(ui, doc, &mut out);
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Game analysis");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                        });
                    });
                    ui.label(RichText::new("Black's perspective").color(MUTED).size(12.0));
                    ui.add_space(12.0);
                    let selected = doc.selected;
                    if let Some(value) = snapshot.values.get(&selected) {
                        ui.columns(2, |columns| {
                            columns[0].label(
                                RichText::new(format!("{:+.1}", value.score_lead))
                                    .size(31.0)
                                    .color(ACCENT),
                            );
                            columns[0].label(
                                RichText::new("points · Black lead").color(MUTED).size(11.0),
                            );
                            columns[1].label(
                                RichText::new(format!("{:.1}%", value.winrate * 100.0)).size(31.0),
                            );
                            columns[1]
                                .label(RichText::new("Black winrate").color(MUTED).size(11.0));
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!(
                                "{} visits · {}",
                                value.visits,
                                if value.visits < 64 {
                                    "provisional"
                                } else {
                                    "refined estimate"
                                }
                            ))
                            .color(MUTED)
                            .size(11.0),
                        );
                    } else {
                        ui.label(RichText::new("Awaiting analysis").size(24.0));
                        ui.label(
                            RichText::new("Explore the board while the chart fills.").color(MUTED),
                        );
                    }
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
                        RichText::new("Click either chart to navigate the played game.")
                            .size(11.0)
                            .color(MUTED),
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
                    ui.label(
                        RichText::new(
                            "← → navigate    P pass    Space pause\n⌘O open    ⌘S export",
                        )
                        .size(11.0)
                        .color(MUTED),
                    );
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
                    ui.label(
                        RichText::new(doc.nodes[0].property("PB").unwrap_or("Black")).strong(),
                    );
                    ui.label(RichText::new("vs").color(MUTED));
                    ui.label(
                        RichText::new(doc.nodes[0].property("PW").unwrap_or("White")).strong(),
                    );
                });
                ui.add_space(9.0);
                board_view(ui, doc, board, &mut out);
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
fn board_view(ui: &mut egui::Ui, doc: &Document, board: &crate::Board, out: &mut Vec<Action>) {
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
fn tree(ui: &mut egui::Ui, doc: &Document, out: &mut Vec<Action>) {
    let layout = doc.layout();
    let max_col = layout.iter().map(|p| p.column).max().unwrap_or(0);
    let max_row = layout.iter().map(|p| p.row).max().unwrap_or(0);
    egui::ScrollArea::both()
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
fn chart(ui: &mut egui::Ui, snapshot: &Snapshot, score: bool, out: &mut Vec<Action>) {
    let Some(doc) = &snapshot.document else {
        return;
    };
    let all = chart_points(snapshot, score);
    let points: Vec<_> = all.iter().flatten().copied().collect();
    let mut plot = Plot::new(if score { "score" } else { "winrate" })
        .height(145.0)
        .include_x(0.0)
        .include_x(doc.mainline.len().saturating_sub(1).max(1) as f64)
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
        let mut segment = Vec::new();
        for point in all.into_iter().chain(std::iter::once(None)) {
            if let Some(point) = point {
                segment.push(point);
            } else if !segment.is_empty() {
                plot_ui.line(
                    Line::new("Played game", std::mem::take(&mut segment))
                        .color(if score {
                            ACCENT
                        } else {
                            Color32::from_rgb(126, 167, 244)
                        })
                        .width(2.0),
                );
            }
        }
        plot_ui.points(
            Points::new("Evaluated positions", points)
                .color(if score {
                    ACCENT
                } else {
                    Color32::from_rgb(126, 167, 244)
                })
                .radius(1.4),
        );
        if let Some(index) = doc.mainline.iter().position(|id| *id == doc.selected)
            && let Some(Analysis {
                score_lead,
                winrate,
                ..
            }) = snapshot.values.get(&doc.selected)
        {
            plot_ui.points(
                Points::new(
                    "Selected",
                    vec![[
                        index as f64,
                        if score { *score_lead } else { *winrate * 100.0 },
                    ]],
                )
                .radius(4.0)
                .color(Color32::WHITE),
            );
        }
        plot_ui.pointer_coordinate()
    });
    if response.response.clicked()
        && let Some(point) = response.inner
    {
        let index = (point.x.round().max(0.0) as usize).min(doc.mainline.len() - 1);
        command(out, Command::Select(doc.mainline[index]));
    }
}
