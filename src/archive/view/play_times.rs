//! Win/loss records for the date and server filters above the Games view.
use crate::archive::play_times::{PlayTimes, Record};
use eframe::egui;

const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const BANDS: [&str; 4] = ["00:00–05:59", "06:00–11:59", "12:00–17:59", "18:00–23:59"];

pub(super) fn show(
    ui: &mut egui::Ui,
    current: &PlayTimes,
    previous: Option<&PlayTimes>,
    by_hour: &mut bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(by_hour, "By hour")
            .on_hover_text("Show each local hour separately.");
        ui.weak("Times are local to your Mac.").on_hover_text(
            "Fox records the start time, OGS records the finish time, and Pandanet supplies its archived game time.",
        );
        for (count, prefix) in [
            (current.unknown_outcome, ""),
            (
                previous.map_or(0, |p| p.unknown_outcome),
                "Previous period · ",
            ),
        ] {
            if count > 0 {
                let unit = if count == 1 { "game" } else { "games" };
                ui.weak(format!("{prefix}{count} {unit} without a win/loss result"));
            }
        }
    });
    let weekdays = |ui: &mut egui::Ui| {
        grid(ui, "Day of the week", previous.is_some(), |ui| {
            for (i, day) in DAYS.iter().enumerate() {
                row(
                    ui,
                    day,
                    current.weekdays[i],
                    previous.map(|p| p.weekdays[i]),
                );
            }
            unknown_row(
                ui,
                "Unknown day",
                current.unknown_day,
                previous.map(|p| p.unknown_day),
            );
        });
    };
    let times = |ui: &mut egui::Ui| {
        grid(ui, "Time of day", previous.is_some(), |ui| {
            if *by_hour {
                for (i, record) in current.hours.iter().enumerate() {
                    row(
                        ui,
                        &format!("{i:02}:00"),
                        *record,
                        previous.map(|p| p.hours[i]),
                    );
                }
            } else {
                let prev = previous.map(PlayTimes::six_hour_blocks);
                for (i, record) in current.six_hour_blocks().into_iter().enumerate() {
                    row(ui, BANDS[i], record, prev.map(|p| p[i]));
                }
            }
            unknown_row(
                ui,
                "Unknown time",
                current.unknown_time,
                previous.map(|p| p.unknown_time),
            );
        });
    };
    let height = ui.available_height()
        - ui.spacing().interact_size.y
        - 3. * ui.spacing().item_spacing.y
        - 6.;
    egui::ScrollArea::vertical()
        .id_salt("game-win-loss-scroll")
        .max_height(height.max(0.))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if ui.available_width() >= if previous.is_some() { 720. } else { 490. } {
                ui.columns(2, |columns| {
                    weekdays(&mut columns[0]);
                    times(&mut columns[1]);
                });
            } else {
                weekdays(ui);
                ui.separator();
                times(ui);
            }
        });
}

fn grid(ui: &mut egui::Ui, title: &str, compare: bool, rows: impl FnOnce(&mut egui::Ui)) {
    ui.heading(title);
    egui::Grid::new(title)
        .striped(true)
        .min_col_width(16.)
        .spacing([12., 5.])
        .show(ui, |ui| {
            for label in ["", "W–L", "Win rate"] {
                ui.weak(label);
            }
            if compare {
                ui.weak("Previous")
                    .on_hover_text("Wins, losses and win rate in the preceding period.");
                ui.weak("Change")
                    .on_hover_text("Change in win rate, in percentage points.");
            }
            ui.end_row();
            rows(ui);
        });
}

fn unknown_row(ui: &mut egui::Ui, label: &str, record: Record, previous: Option<Record>) {
    if record.win_rate().is_some() || previous.and_then(Record::win_rate).is_some() {
        row(ui, label, record, previous);
    }
}

fn percent(rate: Option<f64>) -> String {
    rate.map_or_else(|| "—".into(), |r| format!("{r:.0}%"))
}

fn row(ui: &mut egui::Ui, name: &str, record: Record, previous: Option<Record>) {
    let rate = record.win_rate();
    let text = percent(rate);
    let label = ui.label(name);
    ui.label(format!("{}–{}", record.wins, record.losses));
    if let Some(value) = rate {
        ui.add(
            egui::ProgressBar::new((value / 100.) as f32)
                .desired_width(78.)
                .desired_height(ui.text_style_height(&egui::TextStyle::Body) + 2.)
                .corner_radius(0)
                .text(&text),
        );
    } else {
        ui.weak(&text);
    }
    let mut description = format!(
        "{name} · {} W / {} L · {text} win rate",
        record.wins, record.losses
    );
    if let Some(prev) = previous {
        let prev_rate = prev.win_rate();
        let delta = rate.zip(prev_rate).map_or_else(
            || "—".into(),
            |(a, b)| {
                let change = (a - b).round() as i32;
                if change == 0 {
                    "0 pp".into()
                } else {
                    format!("{change:+} pp")
                }
            },
        );
        ui.label(format!(
            "{}–{}  {}",
            prev.wins,
            prev.losses,
            percent(prev_rate)
        ));
        ui.label(&delta);
        description.push_str(&format!(
            " · Previous {} W / {} L · {delta}",
            prev.wins, prev.losses
        ));
    }
    label.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &description));
    ui.end_row();
}
