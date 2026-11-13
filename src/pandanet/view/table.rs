use crate::pandanet::{Game, Snapshot};
use eframe::egui::{self, Color32, RichText, WidgetInfo, WidgetType};
use egui_extras::{Column, TableBuilder};
use std::{cmp::Ordering, path::PathBuf};

const HEADINGS: [&str; 5] = ["Date", "White", "Black", "Winner", "Win condition"];

pub(super) struct Sort {
    column: usize,
    descending: bool,
}
impl Default for Sort {
    fn default() -> Self {
        Self {
            column: 0,
            descending: true,
        }
    }
}
impl Sort {
    fn select(&mut self, column: usize) {
        if self.column == column {
            self.descending = !self.descending;
        } else {
            self.column = column;
            self.descending = false;
        }
    }
}

struct Row<'a> {
    game: &'a Game,
    cells: [String; 5],
    names: [&'a str; 2],
    condition: u8,
    points: f64,
}
impl<'a> Row<'a> {
    fn new(game: &'a Game) -> Self {
        let (white, _) = player(&game.white);
        let (black, _) = player(&game.black);
        let (winner, condition, kind, points) = outcome(&game.result);
        Self {
            game,
            cells: [
                game.date.clone(),
                game.white.clone(),
                game.black.clone(),
                winner.into(),
                condition,
            ],
            names: [white, black],
            condition: kind,
            points,
        }
    }
    fn compare(&self, other: &Self, column: usize) -> Ordering {
        let primary = match column {
            1 | 2 => self.names[column - 1]
                .to_lowercase()
                .cmp(&other.names[column - 1].to_lowercase()),
            4 => self
                .condition
                .cmp(&other.condition)
                .then_with(|| self.points.total_cmp(&other.points))
                .then_with(|| {
                    self.cells[4]
                        .to_lowercase()
                        .cmp(&other.cells[4].to_lowercase())
                }),
            _ => self.cells[column]
                .to_lowercase()
                .cmp(&other.cells[column].to_lowercase()),
        };
        primary
            .then_with(|| other.game.date.cmp(&self.game.date))
            .then_with(|| self.game.id.cmp(&other.game.id))
    }
}
fn rank(value: &str) -> Option<i32> {
    let value = value.trim_end_matches(['+', '-', '*', '?']);
    let (number, unit) = value.split_at_checked(value.len().checked_sub(1)?)?;
    let number = number.parse::<i32>().ok().filter(|n| *n > 0 && *n < 100)?;
    match unit {
        "k" => Some(-number),
        "d" => Some(number),
        "p" => Some(100 + number),
        _ => None,
    }
}
fn player(value: &str) -> (&str, &str) {
    let value = value.trim();
    if let Some((name, rating)) = value.rsplit_once(char::is_whitespace)
        && (rank(rating).is_some() || matches!(rating, "NR" | "?" | "-"))
    {
        return (name.trim_end(), rating);
    }
    (value, "—")
}
fn outcome(value: &str) -> (&str, String, u8, f64) {
    let value = value.trim();
    let lower = value.to_lowercase();
    let (winner, reason) = if lower.starts_with("black won by ") {
        ("Black", &value[13..])
    } else if lower.starts_with("white won by ") {
        ("White", &value[13..])
    } else if lower.starts_with("b+") {
        ("Black", &value[2..])
    } else if lower.starts_with("w+") {
        ("White", &value[2..])
    } else if matches!(lower.as_str(), "draw" | "jigo" | "0") {
        return ("Draw", "Draw".into(), 4, 0.);
    } else {
        return (
            "—",
            if value.is_empty() {
                "—".into()
            } else {
                value.into()
            },
            5,
            0.,
        );
    };
    let reason = reason.trim();
    if let Ok(points) = reason.parse::<f64>()
        && points.is_finite()
        && points >= 0.
    {
        return (winner, format!("{points} points"), 0, points);
    }
    let (condition, kind) = match reason.to_lowercase().as_str() {
        "r" | "resign" | "resignation" => ("Resignation", 1),
        "t" | "time" | "timeout" | "time out" => ("Time", 2),
        "f" | "forfeit" | "forfeiture" => ("Forfeit", 3),
        _ => (reason, 5),
    };
    (winner, condition.into(), kind, 0.)
}

pub(super) fn show(
    ui: &mut egui::Ui,
    state: &Snapshot,
    query: &str,
    sort: &mut Sort,
) -> Option<PathBuf> {
    let query = query.trim().to_lowercase();
    let mut rows: Vec<_> = state
        .games
        .iter()
        .map(Row::new)
        .filter(|row| {
            query.is_empty()
                || row.cells.join(" ").to_lowercase().contains(&query)
                || row.game.result.to_lowercase().contains(&query)
        })
        .collect();
    let mut open = None;
    if rows.is_empty() {
        ui.label(if state.games.is_empty() {
            "Connect in Sync settings to download games."
        } else {
            "No matching games."
        });
    }
    let width = ui.available_width().max(800.);
    // Reserve the header, row count and the window's close controls; the body
    // consumes the remaining height instead of imposing a fixed pixel cap.
    let body_height = (ui.available_height()
        - 32.
        - ui.text_style_height(&egui::TextStyle::Small)
        - (ui.text_style_height(&egui::TextStyle::Body) + 2. * ui.spacing().button_padding.y)
            .max(ui.spacing().interact_size.y)
        - 6.
        - 5. * ui.spacing().item_spacing.y)
        .max(0.);
    egui::ScrollArea::horizontal()
        .id_salt("library-table-horizontal")
        .content_margin(0.)
        .show(ui, |ui| {
            ui.set_width(width);
            TableBuilder::new(ui)
                .id_salt("synced-games-table-inline-ranks")
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(185.).at_least(180.).clip(true))
                .column(Column::initial(200.).at_least(120.).clip(true))
                .column(Column::initial(200.).at_least(120.).clip(true))
                .column(Column::initial(78.).at_least(65.).clip(true))
                .column(Column::remainder().at_least(130.).clip(true))
                .max_scroll_height(body_height)
                .min_scrolled_height(0.)
                .auto_shrink([false, false])
                .header(32., |mut header| {
                    for (column, heading) in HEADINGS.iter().enumerate() {
                        header.col(|ui| {
                            let active = sort.column == column;
                            let title = format!("{heading}   ");
                            let mut text = RichText::new(title).strong();
                            if active {
                                text = text.color(Color32::from_rgb(95, 206, 175));
                            }
                            let response = ui.add(egui::Button::new(text).frame(false));
                            if active {
                                let center = egui::pos2(
                                    response.rect.right() - 5.,
                                    response.rect.center().y,
                                );
                                let direction = if sort.descending { 1. } else { -1. };
                                ui.painter().add(egui::Shape::convex_polygon(
                                    vec![
                                        center + egui::vec2(-3., -2. * direction),
                                        center + egui::vec2(3., -2. * direction),
                                        center + egui::vec2(0., 2. * direction),
                                    ],
                                    Color32::from_rgb(95, 206, 175),
                                    egui::Stroke::NONE,
                                ));
                            }
                            response.widget_info(|| {
                                WidgetInfo::labeled(
                                    WidgetType::Button,
                                    true,
                                    format!("Sort by {heading}"),
                                )
                            });
                            if response
                                .on_hover_text("Click to sort; click again to reverse")
                                .clicked()
                            {
                                sort.select(column);
                            }
                        });
                    }
                })
                .body(|body| {
                    rows.sort_by(|a, b| {
                        let order = a.compare(b, sort.column);
                        if sort.descending {
                            order.reverse()
                        } else {
                            order
                        }
                    });
                    body.rows(30., rows.len(), |mut row| {
                        let entry = &rows[row.index()];
                        for (column, value) in entry.cells.iter().enumerate() {
                            row.col(|ui| {
                                if column == 0 {
                                    let won = (entry.cells[3] == "White"
                                        && entry.names[0].eq_ignore_ascii_case(&state.username))
                                        || (entry.cells[3] == "Black"
                                            && entry.names[1]
                                                .eq_ignore_ascii_case(&state.username));
                                    trophy(ui, won, &entry.game.date);
                                    let response = ui.add(egui::Button::new(value).frame(false));
                                    response.widget_info(|| {
                                        WidgetInfo::labeled(
                                            WidgetType::Button,
                                            true,
                                            format!(
                                                "Review {}: {} vs {}",
                                                entry.game.date, entry.game.white, entry.game.black
                                            ),
                                        )
                                    });
                                    if response.clicked() {
                                        open = Some(entry.game.path.clone());
                                    }
                                    return;
                                }
                                if matches!(column, 1 | 2) {
                                    let (name, rating) = player(value);
                                    let mut text = egui::text::LayoutJob::default();
                                    let font_id = egui::TextStyle::Body.resolve(ui.style());
                                    text.append(
                                        name,
                                        0.,
                                        egui::TextFormat {
                                            font_id: font_id.clone(),
                                            color: Color32::from_gray(220),
                                            ..Default::default()
                                        },
                                    );
                                    if rating != "—" {
                                        text.append(
                                            &format!(" {rating}"),
                                            0.,
                                            egui::TextFormat {
                                                font_id,
                                                color: Color32::from_gray(165),
                                                ..Default::default()
                                            },
                                        );
                                    }
                                    ui.add(egui::Label::new(text).truncate().selectable(false))
                                        .on_hover_text(value);
                                } else {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(value).color(Color32::from_gray(220)),
                                        )
                                        .truncate()
                                        .selectable(false),
                                    )
                                    .on_hover_text(value);
                                }
                            });
                        }
                        let response = row.response();
                        if response
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            open = Some(entry.game.path.clone());
                        }
                    });
                });
        });
    ui.small(format!(
        "{} of {} games · Click a row to review",
        rows.len(),
        state.games.len()
    ));
    open
}

fn trophy(ui: &mut egui::Ui, won: bool, date: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16., 18.), egui::Sense::hover());
    if !won {
        return;
    }
    let gold = Color32::from_rgb(230, 183, 44);
    let origin = rect.min;
    let point = |x, y| origin + egui::vec2(x, y);
    let painter = ui.painter();
    for points in [
        vec![point(4., 3.), point(1., 3.), point(1., 6.), point(5., 9.)],
        vec![
            point(12., 3.),
            point(15., 3.),
            point(15., 6.),
            point(11., 9.),
        ],
    ] {
        painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, gold)));
    }
    painter.add(egui::Shape::convex_polygon(
        vec![
            point(3., 2.),
            point(13., 2.),
            point(12., 7.),
            point(9., 10.),
            point(7., 10.),
            point(4., 7.),
        ],
        gold,
        egui::Stroke::NONE,
    ));
    painter.rect_filled(
        egui::Rect::from_min_max(point(7., 9.), point(9., 14.)),
        0.,
        gold,
    );
    painter.rect_filled(
        egui::Rect::from_min_max(point(4., 14.), point(12., 16.)),
        1.,
        gold,
    );
    response
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, format!("You won on {date}")));
    response.on_hover_text("You won this game");
}
