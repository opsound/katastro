use super::{ACCENT, Action, command};
use crate::{Color, Document, worker::Command};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, WidgetInfo, WidgetType,
};

pub(super) fn move_buttons(ui: &mut egui::Ui, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 4.0 * ui.spacing().item_spacing.x) / 5.0;
        for (text, label, value) in [
            ("First", "First move", Command::First),
            ("Prev", "Previous move", Command::Step(false)),
            ("Next", "Next move", Command::Step(true)),
            ("Last", "Last move", Command::Last),
            ("Pass", "Pass", Command::Play(None)),
        ] {
            let response = ui.add_sized(Vec2::new(width, 30.0), egui::Button::new(text));
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
            if response.clicked() {
                command(out, value);
            }
            response.on_hover_text(label);
        }
    });
}

pub(super) fn tree(
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
