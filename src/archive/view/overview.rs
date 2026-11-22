//! Summary statistics for the filtered game library.
use crate::archive::{
    categories::Category,
    period::Interval,
    play_times::{PlayTimes, Record},
};
use eframe::egui::{self, WidgetInfo, WidgetType};

#[derive(Default, Clone, Copy)]
struct Rate {
    hits: usize,
    games: usize,
}

impl Rate {
    fn percent(self) -> Option<f64> {
        (self.games > 0).then(|| 100.0 * self.hits as f64 / self.games as f64)
    }

    fn change(self, previous: Self) -> Option<String> {
        let delta = (self.percent()? - previous.percent()?).round() as i32;
        Some(if delta > 0 {
            format!("+{delta} pp")
        } else {
            format!("{delta} pp")
        })
    }

    fn label(self, previous: Option<Self>, singular: &str, plural: &str) -> String {
        let unit = if self.games == 1 { singular } else { plural };
        let rate = match (self.percent(), previous.and_then(|p| self.change(p))) {
            (Some(now), Some(delta)) => format!("{now:.0}%, {delta}"),
            (Some(now), None) => format!("{now:.0}%"),
            (None, _) => "—".into(),
        };
        format!("{}/{} {unit} ({rate})", self.hits, self.games)
    }
}

#[derive(Default)]
pub(super) struct Stats {
    pub(super) times: PlayTimes,
    wins: Rate,
    losses: Rate,
    pending: usize,
    unavailable: usize,
}

impl Stats {
    pub(super) fn count(&mut self, category: Category, won: Option<bool>) {
        match category {
            Category::Pending => self.pending += 1,
            Category::Unavailable => self.unavailable += 1,
            Category::Comeback | Category::Blunder | Category::Other => match won {
                Some(true) => {
                    self.wins.games += 1;
                    self.wins.hits += usize::from(category == Category::Comeback);
                }
                Some(false) => {
                    self.losses.games += 1;
                    self.losses.hits += usize::from(category == Category::Blunder);
                }
                None => self.unavailable += 1,
            },
        }
    }

    pub(super) fn summary(&self, previous: Option<&Self>) -> String {
        let wins = self.wins.label(previous.map(|p| p.wins), "win", "wins");
        let losses = self
            .losses
            .label(previous.map(|p| p.losses), "loss", "losses");
        let mut text = format!("Comebacks {wins} · Blunders {losses}");
        for (count, name) in [(self.pending, "pending"), (self.unavailable, "unavailable")] {
            if count > 0 {
                text.push_str(&format!(" · {count} {name}"));
            }
        }
        text
    }
}

impl From<Record> for Rate {
    fn from(record: Record) -> Self {
        Self {
            hits: record.wins,
            games: record.wins + record.losses,
        }
    }
}

fn metric(
    ui: &mut egui::Ui,
    name: &str,
    rate: Rate,
    previous: Option<Rate>,
    counts: impl Fn(Rate) -> String,
) {
    let percent = |r: Rate| {
        r.percent()
            .map_or_else(|| "—".into(), |v| format!("{v:.0}%"))
    };
    let sample = counts(rate);
    let value = percent(rate);
    ui.strong(name);
    let headline = ui.heading(&value);
    ui.label(&sample);
    let mut description = format!("{name} · {value} · {sample}");
    if let Some(previous) = previous {
        let comparison = counts(previous);
        let change = rate.change(previous);
        let mut caption = format!("Previous {}", percent(previous));
        if let Some(delta) = &change {
            caption.push_str(&format!(" · {delta}"));
        }
        ui.weak(caption).on_hover_text(&comparison);
        let delta = change.as_deref().unwrap_or("—");
        description.push_str(&format!(" · Previous {comparison} · {delta}"));
    }
    headline.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &description));
}

fn overview_summary(ui: &mut egui::Ui, now: &Stats, prior: Option<&Stats>) {
    let classified = |rate: Rate, single, plural| {
        let unit = if rate.games == 1 { single } else { plural };
        format!("{} of {} classified {unit}", rate.hits, rate.games)
    };
    ui.columns(3, |cols| {
        metric(
            &mut cols[0],
            "Win rate",
            now.times.overall().into(),
            prior.map(|p| p.times.overall().into()),
            |r| format!("{} W / {} L", r.hits, r.games - r.hits),
        );
        metric(
            &mut cols[1],
            "Comebacks",
            now.wins,
            prior.map(|p| p.wins),
            |r| classified(r, "win", "wins"),
        );
        metric(
            &mut cols[2],
            "Blunders",
            now.losses,
            prior.map(|p| p.losses),
            |r| classified(r, "loss", "losses"),
        );
    });
    ui.horizontal_wrapped(|ui| {
        for (stats, prefix) in
            std::iter::once((now, "")).chain(prior.map(|p| (p, "Previous period · ")))
        {
            for (n, reason) in [
                (stats.times.unknown_outcome, "without a win/loss result"),
                (stats.pending, "awaiting classification"),
            ] {
                if n > 0 {
                    let unit = if n == 1 { "game" } else { "games" };
                    ui.weak(format!("{prefix}{n} {unit} {reason}"));
                }
            }
        }
    });
}

#[derive(Default)]
pub(super) struct Overview {
    by_hour: bool,
    show_rank_history: bool,
}

impl Overview {
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        current: &Stats,
        previous: Option<(Interval, &Stats)>,
        reading_results: bool,
        rank_chart: impl FnOnce(&mut egui::Ui),
    ) {
        if reading_results {
            ui.weak("Reading game results…");
        }
        if let Some((range, _)) = previous {
            ui.weak(format!("Previous period {range}"));
        }
        let prior = previous.map(|(_, stats)| stats);
        overview_summary(ui, current, prior);
        let prior_times = prior.map(|p| &p.times);
        ui.separator();
        ui.checkbox(&mut self.show_rank_history, "Show rank history");
        if self.show_rank_history {
            rank_chart(ui);
        }
        ui.separator();
        super::play_times::show(ui, &current.times, prior_times, &mut self.by_hour);
    }
}
