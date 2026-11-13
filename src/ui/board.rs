use super::{Action, GROUP_DYING, GROUP_LIVE, GROUP_UNSETTLED, command, suggestion_color};
use crate::{
    Analysis, Color, Document, Point,
    worker::{Command, Snapshot},
};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, WidgetInfo, WidgetType,
};

pub(super) fn board_view(
    ui: &mut egui::Ui,
    doc: &Document,
    board: &crate::Board,
    snapshot: &Snapshot,
    group_strength: bool,
    show_ai_moves: bool,
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
    let side = ui.available_width().min(ui.available_height()).max(140.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Go board"));
    let painter = ui.painter();
    painter.rect_filled(rect, 12, Color32::from_rgb(221, 185, 130));
    let padding = (side * 0.055).max(20.0);
    let start = rect.min + Vec2::splat(padding);
    let extent = side - 2.0 * padding;
    let gap = extent / (doc.size - 1) as f32;
    let marker_radius = gap * 0.43;
    let move_number = |point: Point| {
        snapshot
            .move_numbers
            .get(point.y * doc.size + point.x)
            .copied()
            .flatten()
    };
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
    if group_strength
        && let Some(analysis) = analysis.filter(|a| a.ownership.len() == board.cells.len())
    {
        paint_group_strength(painter, board, &analysis.ownership, start, gap);
    }
    if show_ai_moves && let Some(analysis) = analysis {
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
        let already_labeled = show_ai_moves
            && analysis.is_some_and(|a| {
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
    for (index, color) in board.cells.iter().enumerate() {
        let point = Point::new(index % doc.size, index / doc.size);
        if let Some(color) = color
            && let Some(number) = move_number(point)
        {
            let center = start + Vec2::new(point.x as f32 * gap, point.y as f32 * gap);
            painter.text(
                center,
                Align2::CENTER_CENTER,
                number.to_string(),
                FontId::proportional(gap * 0.42),
                if *color == Color::Black {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(30, 32, 35)
                },
            );
            ui.interact(
                Rect::from_center_size(center, Vec2::splat(gap * 0.6)),
                ui.id().with(("variation-move", index)),
                Sense::hover(),
            )
            .widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Other,
                    true,
                    format!(
                        "Variation move {number}: {} {}",
                        if *color == Color::Black {
                            "Black"
                        } else {
                            "White"
                        },
                        point.gtp(doc.size)
                    ),
                )
            });
        }
    }
    if let Some(point) = doc.nodes[doc.selected].played.and_then(|m| m.point)
        && move_number(point).is_none()
    {
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

fn paint_group_strength(
    painter: &egui::Painter,
    board: &crate::Board,
    ownership: &[f64],
    start: Pos2,
    gap: f32,
) {
    let mut seen = vec![false; board.cells.len()];
    for (index, stone) in board.cells.iter().enumerate() {
        let Some(color) = stone.filter(|_| !seen[index]) else {
            continue;
        };
        let mut chain = Vec::new();
        let mut frontier = vec![index];
        seen[index] = true;
        while let Some(cell) = frontier.pop() {
            chain.push(cell);
            let x = cell % board.size;
            let y = cell / board.size;
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                if let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy))
                    && nx < board.size
                    && ny < board.size
                {
                    let neighbor = ny * board.size + nx;
                    if !seen[neighbor] && board.cells[neighbor] == Some(color) {
                        seen[neighbor] = true;
                        frontier.push(neighbor);
                    }
                }
            }
        }
        let owned = chain.iter().map(|cell| ownership[*cell]).sum::<f64>() / chain.len() as f64
            * if color == Color::Black { 1.0 } else { -1.0 };
        let hue = if owned > 0.5 {
            GROUP_LIVE
        } else if owned < -0.5 {
            GROUP_DYING
        } else {
            GROUP_UNSETTLED
        };
        let decisiveness = ((owned.abs() - 0.5) * 2.0).clamp(0.0, 1.0) as f32;
        let stroke = Stroke::new(1.5, hue);
        let halo = Stroke::new(3.2, Color32::from_rgb(28, 31, 34));
        for cell in chain {
            let x = cell % board.size;
            let y = cell / board.size;
            let center = start + Vec2::new(x as f32 * gap, y as f32 * gap);
            // The same opaque hue stays legible on both Black and White stones.
            painter.circle_stroke(
                center,
                gap * 0.32,
                Stroke::new((gap * 0.10).clamp(2.0, 4.0) * 0.5, hue),
            );
            // Exposed cell edges form the boundary; connected stones have no seam.
            for (dx, dy, a, b) in [
                (-1, 0, Vec2::new(-0.48, -0.48), Vec2::new(-0.48, 0.48)),
                (1, 0, Vec2::new(0.48, -0.48), Vec2::new(0.48, 0.48)),
                (0, -1, Vec2::new(-0.48, -0.48), Vec2::new(0.48, -0.48)),
                (0, 1, Vec2::new(-0.48, 0.48), Vec2::new(0.48, 0.48)),
            ] {
                let connected = x
                    .checked_add_signed(dx)
                    .zip(y.checked_add_signed(dy))
                    .is_some_and(|(nx, ny)| {
                        nx < board.size
                            && ny < board.size
                            && board.cells[ny * board.size + nx] == Some(color)
                    });
                if connected {
                    continue;
                }
                let points = [center + a * gap, center + b * gap];
                for outline in [halo, stroke] {
                    if decisiveness >= 0.9 {
                        painter.line_segment(points, outline);
                    } else {
                        painter.extend(egui::Shape::dashed_line(
                            &points,
                            outline,
                            gap * 0.18,
                            gap * (0.06 + 0.12 * (1.0 - decisiveness)),
                        ));
                    }
                }
            }
        }
    }
}

fn next_recorded_move(doc: &Document) -> Option<(crate::NodeId, crate::Move)> {
    let mut next = doc.continuation(doc.selected);
    while let Some(id) = next {
        if let Some(played) = doc.nodes[id].played {
            return Some((id, played));
        }
        next = doc.continuation(id);
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
