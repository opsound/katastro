use super::{ACCENT, Action, command};
use crate::worker::{Command, Snapshot};
use eframe::egui::{self, Color32, WidgetInfo, WidgetType};
use egui_plot::{Line, Plot, PlotUi, Points};
const CHART_VARIATION: Color32 = Color32::from_rgb(255, 195, 105);

pub fn chart_points(snapshot: &Snapshot, score: bool) -> Vec<Option<[f64; 2]>> {
    let Some(doc) = &snapshot.document else {
        return Vec::new();
    };
    doc.mainline
        .iter()
        .enumerate()
        .map(|(index, id)| assessment(snapshot, *id, score).map(|value| [index as f64, value]))
        .collect()
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

pub(super) fn chart(ui: &mut egui::Ui, snapshot: &Snapshot, score: bool, out: &mut Vec<Action>) {
    let Some(doc) = &snapshot.document else {
        return;
    };
    let all = chart_points(snapshot, score);
    let mut path = doc.mainline.clone();
    let fork = if doc.mainline.contains(&doc.selected) {
        None
    } else {
        path = doc.path(doc.selected).unwrap_or_default();
        while let Some(child) = path.last().and_then(|id| doc.continuation(*id)) {
            path.push(child);
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
        .height(110.0)
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
