use crate::archive::period::{Interval, Period, parse_date};
use eframe::egui;
use time::Date;

#[derive(Default)]
pub(super) struct Controls {
    draft: Option<[String; 2]>,
}

const PRESETS: [(Period, &str); 4] = [
    (Period::Last30, "Last 30 days"),
    (Period::Last90, "Last 90 days"),
    (Period::ThisYear, "This year"),
    (Period::AllTime, "All time"),
];

impl Controls {
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        period: &mut Period,
        compare: &mut bool,
        today: Date,
    ) {
        let label = ui.label("Date range");
        let selected = match *period {
            Period::Custom(range) => range.to_string(),
            preset => PRESETS
                .iter()
                .find(|(p, _)| *p == preset)
                .unwrap()
                .1
                .to_owned(),
        };
        egui::ComboBox::from_id_salt("game-date-range")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (preset, name) in PRESETS {
                    ui.selectable_value(period, preset, name);
                }
                if ui
                    .selectable_label(matches!(period, Period::Custom(_)), "Custom…")
                    .clicked()
                {
                    let initial = period
                        .windows(today)
                        .current
                        .or_else(|| Period::Last90.windows(today).current)
                        .unwrap();
                    self.draft = Some([initial.start.to_string(), initial.end.to_string()]);
                }
            })
            .response
            .labelled_by(label.id);
        ui.add_enabled_ui(*period != Period::AllTime, |ui| {
            ui.checkbox(compare, "Compare previous period");
        });

        let Some([start, end]) = self.draft.as_mut() else {
            return;
        };
        let modal = egui::Modal::new(ui.id().with("custom-game-dates")).show(ui.ctx(), |ui| {
            ui.heading("Custom date range");
            egui::Grid::new("game-date-inputs").show(ui, |ui| {
                for (text, value) in [("Start date", &mut *start), ("End date", &mut *end)] {
                    let label = ui.label(text);
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .desired_width(110.)
                            .hint_text("YYYY-MM-DD"),
                    )
                    .labelled_by(label.id);
                    ui.end_row();
                }
            });
            let selection = match (parse_date(start.trim()), parse_date(end.trim())) {
                (Some(start), Some(end)) => {
                    Interval::new(start, end).ok_or("End date must be on or after the start date.")
                }
                _ => Err("Enter valid dates in YYYY-MM-DD format."),
            };
            if let Err(message) = selection {
                ui.label(message);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(selection.is_ok(), egui::Button::new("Apply dates"))
                    .clicked()
                    && let Ok(range) = selection
                {
                    *period = Period::Custom(range);
                    ui.close();
                }
                if ui.button("Cancel").clicked() {
                    ui.close();
                }
            });
        });
        if modal.should_close() {
            self.draft = None;
        }
    }
}
