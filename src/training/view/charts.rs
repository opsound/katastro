use crate::training::Comparison;
use eframe::egui::{self, Color32, WidgetInfo, WidgetType};
use egui_plot::{Legend, Line, Plot};

pub(super) fn show(ui: &mut egui::Ui, comparison: &Comparison) {
    ui.label("Your perspective · moves by both players from the exercise position");
    if !comparison.complete() {
        ui.label("Finishing comparison…");
    }
    ui.label("Score lead");
    let response = Plot::new("training-score-comparison")
        .height(ui.available_height().max(60.0))
        .include_x(0.0)
        .include_x(
            comparison
                .total_plies
                .max(comparison.attempt.len().saturating_sub(1))
                .max(comparison.recorded.len().saturating_sub(1)) as f64,
        )
        .include_y(0.0)
        .legend(Legend::default())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .show(ui, |plot| {
            for (label, values, color) in [
                (
                    "Recorded",
                    &comparison.recorded,
                    Color32::from_rgb(126, 167, 244),
                ),
                (
                    "Attempt",
                    &comparison.attempt,
                    Color32::from_rgb(90, 190, 156),
                ),
            ] {
                let points = values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| [i as f64, v.score])
                    .collect::<Vec<_>>();
                plot.line(Line::new(label, points).color(color).width(1.5));
            }
        });
    response
        .response
        .widget_info(|| WidgetInfo::labeled(WidgetType::Other, true, "Training score comparison"));
}
