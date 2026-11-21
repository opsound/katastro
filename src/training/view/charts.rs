use crate::training::Comparison;
use eframe::egui::{self, Color32, WidgetInfo, WidgetType};
use egui_plot::{AxisHints, GridInput, Legend, Line, Plot, uniform_grid_spacer};

fn score_grid_steps(grid: GridInput) -> [f64; 3] {
    // Leave about 2.5 minimum grid spacings per label and round up to 1, 2 or 5 × 10ⁿ.
    let minimum = grid.base_step_size * 2.5;
    let decade = 10.0_f64.powf(minimum.log10().ceil());
    let step = if minimum <= 0.2 * decade {
        0.2 * decade
    } else if minimum <= 0.5 * decade {
        0.5 * decade
    } else {
        decade
    };
    [step, 2.0 * step, 10.0 * step]
}

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
        .y_grid_spacer(uniform_grid_spacer(score_grid_steps))
        .custom_y_axes(vec![
            AxisHints::new_y()
                .label("Points")
                .min_thickness(58.0)
                .label_spacing(12.0..=18.0)
                .tick_label_color(ui.visuals().text_color())
                .formatter(|mark, _| {
                    if mark.value == 0.0 {
                        return "0".into();
                    }
                    let decimals = -mark.step_size.log10().floor() as usize;
                    format!("{:+.decimals$}", mark.value)
                }),
        ])
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
