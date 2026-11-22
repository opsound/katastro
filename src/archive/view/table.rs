use super::overview::{Overview, Stats};
use crate::{
    Color,
    archive::{
        Game, Server, Snapshot,
        categories::{Category, Evidence, Thresholds, winner_and_reason},
        period::{Bucket, Windows},
        rank_history::player,
    },
};
use eframe::egui::{self, Color32, RichText, WidgetInfo, WidgetType};
use egui_extras::{Column, TableBuilder};
use std::{cmp::Ordering, collections::HashMap, path::PathBuf};

const HEADINGS: [&str; 6] = [
    "Date",
    "White",
    "Black",
    "Winner",
    "Win condition",
    "Server",
];

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
    cells: [String; 6],
    account: &'a str,
    names: [&'a str; 2],
    condition: u8,
    points: f64,
    category: Category,
}
impl<'a> Row<'a> {
    fn new(game: &'a Game, state: &'a Snapshot, category: Category) -> Self {
        let (white, _) = player(&game.white);
        let (black, _) = player(&game.black);
        let (winner, condition, kind, points) = outcome(&game.result);
        Self {
            game,
            account: &state.username,
            cells: [
                game.date.clone(),
                game.white.clone(),
                game.black.clone(),
                winner.into(),
                condition,
                state.server.label().into(),
            ],
            names: [white, black],
            condition: kind,
            points,
            category,
        }
    }
    fn matches_query(&self, query: &str) -> bool {
        query.is_empty()
            || self.cells.join(" ").to_lowercase().contains(query)
            || self.game.result.to_lowercase().contains(query)
    }

    fn show_cell(&self, ui: &mut egui::Ui, column: usize) -> bool {
        let value = &self.cells[column];
        if column == 0 {
            let won = (self.cells[3] == "White"
                && self.names[0].eq_ignore_ascii_case(self.account))
                || (self.cells[3] == "Black" && self.names[1].eq_ignore_ascii_case(self.account));
            trophy(ui, won, &self.game.date);
            let response = ui.add(egui::Button::new(value).frame(false));
            response.widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Button,
                    true,
                    format!(
                        "Review {}: {} vs {}",
                        self.game.date, self.game.white, self.game.black
                    ),
                )
            });
            return response
                .on_hover_text(match self.category {
                    Category::Comeback => "Comeback win",
                    Category::Blunder => "Blunder loss",
                    Category::Other => "Classified game below the selected threshold",
                    Category::Pending => "Category pending analysis",
                    Category::Unavailable => "Game category unavailable",
                })
                .clicked();
        }
        if matches!(column, 1 | 2) {
            let (name, rating) = player(value);
            let mut text = egui::text::LayoutJob::default();
            let suffix = if rating == "—" {
                String::new()
            } else {
                format!(" {rating}")
            };
            for (part, gray) in [(name, 220), (suffix.as_str(), 165)] {
                text.append(
                    part,
                    0.,
                    egui::TextFormat {
                        font_id: egui::TextStyle::Body.resolve(ui.style()),
                        color: Color32::from_gray(gray),
                        ..Default::default()
                    },
                );
            }
            ui.add(egui::Label::new(text).truncate().selectable(false))
                .on_hover_text(value);
        } else {
            ui.add(
                egui::Label::new(RichText::new(value).color(Color32::from_gray(220)))
                    .truncate()
                    .selectable(false),
            )
            .on_hover_text(value);
        }
        false
    }
    fn compare(&self, other: &Self, column: usize) -> Ordering {
        let date_order = self
            .game
            .timestamp
            .cmp(&other.game.timestamp)
            .then_with(|| self.game.date.cmp(&other.game.date));
        let primary = match column {
            0 => date_order,
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
            .then(date_order.reverse())
            .then_with(|| self.game.id.cmp(&other.game.id))
    }
}
pub(super) fn matches_search(game: &Game, state: &Snapshot, query: &str) -> bool {
    query.is_empty() || Row::new(game, state, Category::Pending).matches_query(query)
}

fn outcome(value: &str) -> (&str, String, u8, f64) {
    let value = value.trim();
    let Some((color, reason)) = winner_and_reason(value) else {
        return match value.to_ascii_lowercase().as_str() {
            "draw" | "jigo" | "0" => ("Draw", "Draw".into(), 4, 0.),
            "" => ("—", "—".into(), 5, 0.),
            _ => ("—", value.into(), 5, 0.),
        };
    };
    let winner = match color {
        Color::Black => "Black",
        Color::White => "White",
    };
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

pub(super) struct Filter<'a> {
    pub server: Option<Server>,
    pub search: &'a str,
    pub category: Option<Category>,
    pub evidence: &'a HashMap<PathBuf, Evidence>,
    pub thresholds: Thresholds,
    pub period: Windows,
    pub compare: bool,
}

pub(super) fn show(
    ui: &mut egui::Ui,
    states: &[&Snapshot],
    sort: &mut Sort,
    filter: Filter<'_>,
    overview: Option<&mut Overview>,
) -> Option<PathBuf> {
    let query = filter.search.trim().to_lowercase();
    let total = states.iter().map(|s| s.games.len()).sum::<usize>();
    let mut rows = Vec::new();
    let mut current = Stats::default();
    let mut previous = Stats::default();
    let mut reading_results = false;
    let previous_period = filter.period.previous.filter(|_| filter.compare);
    for state in states
        .iter()
        .filter(|s| filter.server.is_none_or(|f| s.server == f))
    {
        for game in &state.games {
            let Some(bucket) = filter.period.bucket(&game.date) else {
                continue;
            };
            if bucket == Bucket::Previous && previous_period.is_none() {
                continue;
            }
            let evidence = filter.evidence.get(&game.path).copied().unwrap_or_default();
            let category = evidence.category(filter.thresholds);
            let show_row = overview.is_none()
                && bucket == Bucket::Current
                && filter.category.is_none_or(|c| c == category);
            if show_row || !query.is_empty() {
                let row = Row::new(game, state, category);
                if !row.matches_query(&query) {
                    continue;
                }
                if show_row {
                    rows.push(row);
                }
            }
            let stats = match bucket {
                Bucket::Current => &mut current,
                Bucket::Previous => &mut previous,
            };
            if overview.is_some() {
                if matches!(evidence, Evidence::Pending) {
                    reading_results = true;
                    continue;
                }
                stats.times.count_game(&game.date, evidence);
            }
            stats.count(category, evidence.won());
        }
    }
    if let Some(overview) = overview {
        overview.show(
            ui,
            &current,
            previous_period.map(|range| (range, &previous)),
            reading_results,
            |ui| super::rank_chart::show(ui, states, filter.server, filter.period, &query),
        );
        return None;
    }
    if total > 0 {
        ui.horizontal_wrapped(|ui| {
            ui.small(format!("{} of {} games · Click a row to review", rows.len(), total));
            ui.separator();
            ui.label(current.summary(previous_period.map(|_| &previous)))
                .on_hover_text("Rates use classified wins and losses. Changes are measured in percentage points from the previous period.");
        });
        if let Some(range) = previous_period {
            ui.label(format!("Previous {range} · {}", previous.summary(None)));
        }
    }
    let mut open = None;
    if rows.is_empty() {
        ui.label(if total == 0 {
            "Connect in Sync settings to download games."
        } else {
            "No matching games."
        });
    }
    let width = ui.available_width().max(800.);
    // Reserve the header and the window's close controls; the body
    // consumes the remaining height instead of imposing a fixed pixel cap.
    let body_height = (ui.available_height()
        - 32.
        - (ui.text_style_height(&egui::TextStyle::Body) + 2. * ui.spacing().button_padding.y)
            .max(ui.spacing().interact_size.y)
        - 6.
        - 4. * ui.spacing().item_spacing.y)
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
                .column(Column::initial(150.).at_least(130.).clip(true))
                .column(Column::remainder().at_least(80.).clip(true))
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
                        for column in 0..HEADINGS.len() {
                            row.col(|ui| {
                                if entry.show_cell(ui, column) {
                                    open = Some(entry.game.path.clone());
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
