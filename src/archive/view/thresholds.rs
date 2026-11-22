//! Edit the point gains used to classify losses and wins.
use crate::archive::categories::Thresholds;
use eframe::egui;

pub(super) struct Editor {
    comeback: String,
    blunder: String,
}

impl From<Thresholds> for Editor {
    fn from(value: Thresholds) -> Self {
        Self {
            comeback: value.comeback.to_string(),
            blunder: value.blunder.to_string(),
        }
    }
}

pub(super) fn show(ui: &mut egui::Ui, draft: &mut Option<Editor>, thresholds: &mut Thresholds) {
    if ui.button("Thresholds").clicked() {
        *draft = Some((*thresholds).into());
    }
    let Some(editor) = draft else { return };
    let modal = egui::Modal::new(ui.id().with("game-thresholds")).show(ui.ctx(), |ui| {
        ui.heading("Comeback and blunder thresholds");
        ui.label("Measure gains from the opening score.");
        egui::Grid::new("game-threshold-fields").show(ui, |ui| {
            for (name, text, meaning) in [
                (
                    "Comeback threshold",
                    &mut editor.comeback,
                    "points gained by your opponent in a win",
                ),
                (
                    "Blunder threshold",
                    &mut editor.blunder,
                    "points gained by you in a loss",
                ),
            ] {
                let label = ui.label(name);
                ui.add(egui::TextEdit::singleline(text).desired_width(72.))
                    .labelled_by(label.id);
                ui.label(meaning);
                ui.end_row();
            }
        });
        let positive = |text: &str| {
            text.trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n > 0.)
        };
        let values = positive(&editor.comeback).zip(positive(&editor.blunder));
        if values.is_none() {
            ui.label("Enter a point value greater than zero for each threshold.");
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(values.is_some(), egui::Button::new("Apply thresholds"))
                .clicked()
                && let Some((comeback, blunder)) = values
            {
                *thresholds = Thresholds { comeback, blunder };
                ui.close();
            }
            if ui.button("Cancel").clicked() {
                ui.close();
            }
            if ui.button("Restore defaults").clicked() {
                *editor = Thresholds::default().into();
            }
        });
    });
    if modal.should_close() {
        *draft = None;
    }
}
