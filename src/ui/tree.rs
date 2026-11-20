use super::{ACCENT, Action, VARIATION_COLOR, caption, command};
use crate::{Color, Document, NodePosition, worker::Command};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, WidgetInfo, WidgetType,
};

pub(super) fn move_menu_id() -> egui::Id {
    egui::Id::new("review-move-actions")
}

fn header_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    paint: impl FnOnce(&egui::Painter, Pos2, Color32),
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::new("")
            .frame_when_inactive(false)
            .min_size(Vec2::new(28.0, 26.0)),
    );
    let color = ui.style().interact(&response).text_color();
    paint(
        ui.painter(),
        response.rect.center(),
        if enabled {
            color
        } else {
            ui.visuals().disable(color)
        },
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    response.on_hover_text(label)
}

fn move_menu(ui: &mut egui::Ui, doc: &Document, out: &mut Vec<Action>) {
    let more = header_button(ui, "More move actions", true, |painter, center, color| {
        for dx in [-5.0, 0.0, 5.0] {
            painter.circle_filled(center + egui::vec2(dx, 0.0), 1.45, color);
        }
    });
    let menu = egui::Popup::menu(&more)
        .id(move_menu_id())
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| {
            ui.set_min_width(160.0);
            ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
            for (text, label, key, enabled, value) in [
                (
                    "First position",
                    "First move",
                    "Home",
                    doc.selected != 0,
                    Command::First,
                ),
                (
                    "Last played move",
                    "Last move",
                    "End",
                    doc.mainline.last() != Some(&doc.selected),
                    Command::Last,
                ),
                ("Play a pass", "Pass", "P", true, Command::Play(None)),
            ] {
                if matches!(value, Command::Play(None)) {
                    ui.separator();
                }
                let response = ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(key));
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
                if response.clicked() {
                    command(out, value);
                    ui.close();
                }
            }
        });
    ui.ctx().accesskit_node_builder(more.id, |node| {
        node.set_has_popup(egui::accesskit::HasPopup::Menu);
        node.set_expanded(menu.is_some());
    });
}

fn tree_header(ui: &mut egui::Ui, doc: &Document, move_number: usize, out: &mut Vec<Action>) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(caption("VARIATIONS").strong())
                .on_hover_text("Played game on top");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().button_padding = Vec2::new(3.0, 0.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                move_menu(ui, doc, out);
                let selected = &doc.nodes[doc.selected];
                for (label, forward, enabled) in [
                    ("Next move", true, !selected.children.is_empty()),
                    ("Previous move", false, selected.parent.is_some()),
                ] {
                    let response = header_button(ui, label, enabled, |painter, center, color| {
                        let x = if forward { 2.7 } else { -2.7 };
                        painter.add(egui::Shape::line(
                            vec![
                                center + egui::vec2(-x, -5.0),
                                center + egui::vec2(x, 0.0),
                                center + egui::vec2(-x, 5.0),
                            ],
                            Stroke::new(1.7, color),
                        ));
                    });
                    if response.clicked() {
                        command(out, Command::Step(forward));
                    }
                }
                ui.add_space(4.0);
                ui.label(caption(format!("Move {move_number}")).size(12.0));
            });
        },
    );
}

const COLUMN_PITCH: f32 = 36.0;
const ROW_PITCH: f32 = 38.0;
const STONE_RADIUS: f32 = 10.0;

fn node_bounds(center: Pos2) -> Rect {
    Rect::from_center_size(center + Vec2::new(0.0, 6.0), Vec2::new(34.0, 36.0))
}

fn paint_edge(ui: &egui::Ui, from: Pos2, to: Pos2, stroke: Stroke) {
    if !ui.is_rect_visible(Rect::from_two_pos(from, to).expand(stroke.width)) {
        return;
    }
    if from.y == to.y {
        ui.painter().line_segment([from, to], stroke);
    } else {
        let gutter = (from.x + to.x) * 0.5;
        let bend = (gutter - from.x).min((to.y - from.y) * 0.5);
        let turn = |points| {
            egui::epaint::QuadraticBezierShape::from_points_stroke(
                points,
                false,
                Color32::TRANSPARENT,
                stroke,
            )
            .flatten(None)
        };
        // Both turns meet the shared stem vertically. Stroke them together to avoid seams.
        let mut points = turn([
            from,
            Pos2::new(gutter, from.y),
            Pos2::new(gutter, from.y + bend),
        ]);
        points.extend(turn([
            Pos2::new(gutter, to.y - bend),
            Pos2::new(gutter, to.y),
            to,
        ]));
        ui.painter().add(egui::Shape::line(points, stroke));
    }
}

pub(super) fn tree(
    ui: &mut egui::Ui,
    doc: &Document,
    source: &Option<std::path::PathBuf>,
    out: &mut Vec<Action>,
) {
    let layout = doc.layout();
    tree_header(ui, doc, layout[doc.selected].move_number, out);
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
                    (max_col as f32 * COLUMN_PITCH + 42.0).max(ui.available_width()),
                    max_row as f32 * ROW_PITCH + 44.0,
                ),
                Sense::hover(),
            );
            let position = |id: usize| {
                rect.min
                    + Vec2::new(
                        21.0 + layout[id].column as f32 * COLUMN_PITCH,
                        16.0 + layout[id].row as f32 * ROW_PITCH,
                    )
            };
            if follow {
                ui.scroll_to_rect_animation(
                    node_bounds(position(doc.selected)).expand(3.0),
                    None,
                    egui::style::ScrollAnimation::none(),
                );
            }
            for p in &layout {
                if let Some(parent) = doc.nodes[p.id].parent {
                    let stroke = if p.row == 0 {
                        Stroke::new(2.0, Color32::from_rgb(144, 157, 168))
                    } else {
                        Stroke::new(1.5, Color32::from_rgb(98, 113, 126))
                    };
                    paint_edge(ui, position(parent), position(p.id), stroke);
                }
            }
            // Draw the selected route last so a sibling sharing its stem can't obscure it.
            let mut child = doc.selected;
            while layout[child].row > 0 {
                let Some(parent) = doc.nodes[child].parent else {
                    break;
                };
                paint_edge(
                    ui,
                    position(parent),
                    position(child),
                    Stroke::new(2.5, VARIATION_COLOR),
                );
                child = parent;
            }
            for p in &layout {
                if tree_node(ui, doc, p, position(p.id)) {
                    command(out, Command::Select(p.id));
                }
            }
        });
    ui.interact(
        output.inner_rect,
        ui.id().with("timeline-viewport"),
        Sense::hover(),
    )
    .widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Timeline viewport"));
}

fn tree_node(ui: &egui::Ui, doc: &Document, p: &NodePosition, center: Pos2) -> bool {
    let hit = node_bounds(center);
    if !ui.is_rect_visible(hit) {
        return false;
    }
    let response = ui.interact(hit, ui.id().with(("node", p.id)), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, true, format!("Tree node {}", p.id))
    });

    let node = &doc.nodes[p.id];
    let selected = doc.selected == p.id;
    let painter = ui.painter();
    if selected {
        painter.circle_stroke(center, STONE_RADIUS + 2.0, Stroke::new(1.5, ACCENT));
    }
    let (caption, detail) = if let Some(played) = node.played {
        let (fill, ink) = match played.color {
            Color::Black => (Color32::from_rgb(25, 28, 32), Color32::WHITE),
            Color::White => (Color32::from_rgb(235, 237, 234), Color32::from_gray(30)),
        };
        painter.circle(
            center,
            STONE_RADIUS,
            fill,
            Stroke::new(1.0, Color32::from_rgb(130, 140, 149)),
        );
        let number = p.move_number.to_string();
        let size = 11.0_f32.min(31.0 / number.len() as f32);
        painter.text(
            center,
            Align2::CENTER_CENTER,
            number,
            FontId::proportional(size),
            ink,
        );
        let caption = played
            .point
            .map(|point| point.gtp(doc.size))
            .unwrap_or_else(|| "Pass".into());
        let detail = format!("Move {} · {} {caption}", p.move_number, played.color);
        (caption, detail)
    } else {
        painter.rect_filled(
            Rect::from_center_size(center, Vec2::splat(9.0)),
            2,
            Color32::from_rgb(145, 157, 168),
        );
        let caption = if ["AB", "AW", "AE", "PL"]
            .iter()
            .any(|key| node.props.contains_key(*key))
        {
            "Setup"
        } else if node.parent.is_none() {
            "Start"
        } else if node.property("C").is_some() || node.property("N").is_some() {
            "Note"
        } else {
            "Node"
        };
        (caption.to_owned(), caption.to_owned())
    };
    painter.text(
        center + Vec2::new(0.0, 12.0),
        Align2::CENTER_TOP,
        &caption,
        FontId::proportional(10.0),
        if selected {
            ACCENT
        } else {
            Color32::from_rgb(191, 200, 209)
        },
    );

    let line = if p.row == 0 {
        "Played game"
    } else {
        "Variation"
    };
    let description = format!("{detail} · {line}");
    ui.ctx().accesskit_node_builder(response.id, |builder| {
        builder.set_description(description.as_str());
        builder.set_selected(selected);
    });
    response.on_hover_text(description).clicked()
}
