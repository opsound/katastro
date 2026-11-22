//! Recorded account ranks, aligned to the calendar dates in the archive.
use crate::archive::{
    Server, Snapshot,
    rank_history::{rank_label, timestamp_label},
};
use eframe::egui::{self, Color32, WidgetInfo, WidgetType};
use egui_plot::{AxisHints, HoverPosition, Line, Plot, PlotPoint, Points, uniform_grid_spacer};
use std::path::Path;

const HOUR: f64 = 3600.;
const DAY: f64 = 24. * HOUR;

// Wider traces are drawn first so matching ranks on the same dates remain visible.
fn style(server: Server) -> (Color32, f32, f32) {
    match server {
        Server::Pandanet => (Color32::from_rgb(126, 169, 240), 3.8, 4.5),
        Server::Fox => (Color32::from_rgb(239, 181, 108), 2.6, 3.3),
        Server::Ogs => (Color32::from_rgb(109, 212, 181), 1.5, 2.2),
    }
}

pub(super) fn show(ui: &mut egui::Ui, states: &[(&Snapshot, &Path)], filter: Option<Server>) {
    let mut series = Vec::new();
    for server in [Server::Pandanet, Server::Fox, Server::Ogs] {
        if let Some((state, _)) = states.iter().find(|(s, _)| {
            s.server == server && filter.is_none_or(|f| server == f) && !s.rank_history.is_empty()
        }) {
            let points: Vec<_> = state
                .rank_history
                .iter()
                .map(|p| PlotPoint::new(p.timestamp as f64, p.level))
                .collect();
            series.push((server, points));
        }
    }
    if series.is_empty() {
        ui.weak("No recorded player ranks in these games.");
        return;
    }

    let mut first = f64::INFINITY;
    let mut last = f64::NEG_INFINITY;
    let mut lowest = f64::INFINITY;
    let mut highest = f64::NEG_INFINITY;
    for (_, points) in &series {
        for p in points {
            first = first.min(p.x);
            last = last.max(p.x);
            lowest = lowest.min(p.y);
            highest = highest.max(p.y);
        }
    }
    let intraday = last - first < 2. * DAY;
    ui.horizontal_wrapped(|ui| {
        ui.strong("Rank history")
            .on_hover_text("Each point shows your rank recorded in a synced game on that server.");
        ui.add_space(6.);
        for (server, points) in &series {
            let level = points.last().unwrap().y as i32;
            ui.colored_label(
                style(*server).0,
                format!("{}  {}", server.label(), rank_label(level)),
            )
            .on_hover_text("Latest recorded rank");
        }
    });

    let x_padding = (HOUR - (last - first)).max(0.) * 0.5;
    let y_padding = if lowest == highest { 2. } else { 0.5 };
    let height = (ui.available_height() * 0.34).clamp(98., if first == last { 114. } else { 160. });
    let color = ui.visuals().text_color();
    let response = Plot::new("library-rank-history")
        .height(height)
        .include_x(first - x_padding)
        .include_x(last + x_padding)
        .include_y(lowest - y_padding)
        .include_y(highest + y_padding)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_axis_zoom_drag(false)
        .x_grid_spacer(uniform_grid_spacer(move |g| {
            let minimum = (g.base_step_size * 17.).max(if intraday { 60. } else { DAY });
            let step = [
                60.,
                300.,
                900.,
                HOUR,
                3. * HOUR,
                6. * HOUR,
                12. * HOUR,
                DAY,
                2. * DAY,
                7. * DAY,
                14. * DAY,
                30. * DAY,
                90. * DAY,
                180. * DAY,
            ]
            .into_iter()
            .find(|s| *s >= minimum)
            .unwrap_or_else(|| (minimum / (365. * DAY)).ceil() * 365. * DAY);
            [step, 2. * step, 10. * step]
        }))
        .custom_x_axes(vec![
            AxisHints::new_x()
                .min_thickness(19.)
                .tick_label_color(color)
                .formatter(move |mark, _| timestamp_label(mark.value.round() as i64, intraday)),
        ])
        .y_grid_spacer(uniform_grid_spacer(|g| {
            // Whole ranks are two half-rank levels apart.
            let step = 2. * (g.base_step_size * 1.3).ceil().max(1.);
            [step, 2. * step, 5. * step]
        }))
        .custom_y_axes(vec![
            AxisHints::new_y()
                .min_thickness(32.)
                .label_spacing(12.0..=18.0)
                .tick_label_color(color)
                .formatter(|mark, _| rank_label(mark.value.round() as i32)),
        ])
        .label_formatter(|pos| match pos {
            HoverPosition::NearDataPoint {
                plot_name,
                position,
                ..
            } => {
                let t = position.x.round() as i64;
                Some(format!(
                    "{}  {}\n{}",
                    plot_name,
                    rank_label(position.y.round() as i32),
                    timestamp_label(t, true)
                ))
            }
            HoverPosition::Elsewhere { .. } => None,
        })
        .show(ui, |plot| {
            for (server, points) in &series {
                let name = server.label();
                let (color, width, radius) = style(*server);
                if points.len() > 1 {
                    plot.line(
                        Line::new(name, points.as_slice())
                            .color(color)
                            .width(width)
                            .allow_hover(false),
                    );
                }
                plot.points(
                    Points::new(name, points.as_slice())
                        .radius(radius)
                        .color(color),
                );
            }
        });
    response
        .response
        .widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Rank history chart"));
}
