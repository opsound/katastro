use super::{
    Command, Server, Snapshot, StatisticsSettings,
    categories::{Category, Evidence, Thresholds},
};
use eframe::egui;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};
mod period;
mod play_times;
mod rank_chart;
mod table;
mod thresholds;
#[derive(Default)]
pub struct View {
    pub open: bool,
    filter: Option<Server>,
    category: Option<Category>,
    pub(crate) categories: HashMap<PathBuf, Evidence>,
    pub(crate) statistics: Option<StatisticsSettings>,
    period_controls: period::Controls,
    threshold_editor: Option<thresholds::Editor>,
    server: Server,
    settings_open: bool,
    show_rank_history: bool,
    show_play_times: bool,
    by_hour: bool,
    username: String,
    password: Zeroizing<String>,
    remember: bool,
    search: String,
    sort: table::Sort,
}
pub struct Actions {
    pub command: Option<(Server, Command)>,
    pub review: Option<PathBuf>,
    pub train: bool,
}
fn limit_label(limit: Option<usize>) -> String {
    limit.map_or_else(|| "All games".into(), |n| format!("Latest {n} games"))
}
impl View {
    pub fn show(&mut self, state: &Snapshot) {
        self.password.zeroize();
        self.server = state.server;
        self.username = state.username.clone();
        self.remember = state.remember;
        self.settings_open = true;
        self.open = true;
    }
    pub fn show_library(&mut self) {
        self.open = true;
        self.settings_open = false;
    }
    pub fn training_server(&self) -> Server {
        self.filter.unwrap_or(self.server)
    }
    fn game_filters(
        &mut self,
        ui: &mut egui::Ui,
        states: &[(&Snapshot, &Path)],
        has_games: bool,
        thresholds: Thresholds,
    ) {
        ui.selectable_value(&mut self.show_play_times, false, "Game list");
        ui.selectable_value(&mut self.show_play_times, true, "Win/loss by time");
        ui.separator();
        let filters =
            std::iter::once(None).chain(states.iter().map(|(snapshot, _)| Some(snapshot.server)));
        for filter in filters {
            let text = filter.map_or("All servers", Server::label);
            let response = ui.selectable_label(self.filter == filter, text);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::SelectableLabel,
                    true,
                    if filter.is_some() {
                        format!("{text} games")
                    } else {
                        text.into()
                    },
                )
            });
            if response.clicked() {
                self.filter = filter;
                if let Some((other, _)) = states.iter().find(|(s, _)| Some(s.server) == filter) {
                    let settings_open = self.settings_open;
                    self.show(other);
                    self.settings_open = settings_open;
                }
            }
        }
        if has_games && !self.show_play_times {
            ui.separator();
            ui.selectable_value(&mut self.category, None, "All categories");
            for (category, name, who, gain) in [
                (
                    Category::Comeback,
                    "Comebacks",
                    "Your opponent’s gain in a game you won",
                    thresholds.comeback,
                ),
                (
                    Category::Blunder,
                    "Blunders",
                    "Your gain in a game you lost",
                    thresholds.blunder,
                ),
            ] {
                ui.selectable_value(&mut self.category, Some(category), name)
                    .on_hover_text(format!(
                        "{who}, at least {gain} points from the opening score."
                    ));
            }
        }
    }
    pub fn render(&mut self, ctx: &egui::Context, states: &[(&Snapshot, &Path)]) -> Actions {
        let mut actions = Actions {
            command: None,
            review: None,
            train: false,
        };
        let mut open = self.open;
        let mut close = false;
        egui::Window::new("Game library")
            .open(&mut open)
            .default_width(1000.)
            .default_height((ctx.content_rect().height() - 40.).clamp(200., 720.))
            .show(ctx, |ui| {
                let Some((state, folder)) = states
                    .iter()
                    .find(|(s, _)| s.server == self.server)
                    .or_else(|| states.first())
                else {
                    return;
                };
                self.server = state.server;
                let has_games = states
                    .iter()
                    .any(|(s, _)| self.filter.is_none_or(|f| f == s.server) && !s.games.is_empty());

                ui.horizontal(|ui| {
                    ui.heading(format!(
                        "{} synced games",
                        states.iter().map(|(s, _)| s.games.len()).sum::<usize>()
                    ));
                    if ui
                        .add_enabled(has_games, egui::Button::new("Train from these games"))
                        .clicked()
                    {
                        actions.train = true;
                        close = true;
                    }
                    if ui.button("Sync settings").clicked() {
                        if self.settings_open {
                            self.settings_open = false;
                        } else {
                            self.show(state);
                        }
                    }
                    if state.syncing {
                        if ui
                            .button(format!("Pause {} sync", state.server.label()))
                            .clicked()
                        {
                            actions.command = Some((state.server, Command::SetAutomatic(false)));
                        }
                    } else if ui
                        .add_enabled(
                            !state.username.is_empty(),
                            egui::Button::new(format!("Sync {} now", state.server.label())),
                        )
                        .clicked()
                    {
                        actions.command = Some((state.server, Command::SyncNow));
                    }
                    if !self.settings_open && has_games && !self.show_play_times {
                        ui.checkbox(&mut self.show_rank_history, "Show rank history");
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(format!("{} history", state.server.label()));
                    let mut limit = state.limit;
                    egui::ComboBox::from_id_salt("pandanet-history-limit")
                        .selected_text(limit_label(limit))
                        .show_ui(ui, |ui| {
                            for choice in
                                [Some(50), Some(100), Some(200), Some(500), Some(1000), None]
                            {
                                ui.selectable_value(&mut limit, choice, limit_label(choice));
                            }
                        });
                    if limit != state.limit {
                        actions.command = Some((state.server, Command::SetLimit(limit)));
                    }
                    ui.small("Older downloads are kept.");
                    if !self.settings_open && !state.syncing {
                        ui.separator();
                        ui.label(&state.status);
                    }
                });
                if self.settings_open {
                    ui.separator();
                    ui.horizontal(|ui| {
                        for (other, _) in states {
                            if ui
                                .selectable_label(
                                    self.server == other.server,
                                    format!("{} sync", other.server.label()),
                                )
                                .clicked()
                            {
                                self.show(other);
                            }
                        }
                        ui.small("Checks every 15 minutes while Katastro is open.");
                    });
                    let needs_password = state.server == Server::Pandanet;
                    if !needs_password {
                        ui.small("Public game records · only your username is needed.");
                    }
                    egui::Grid::new("pandanet-login")
                        .num_columns(2)
                        .show(ui, |ui| {
                            let username_label = format!("{} username", state.server.label());
                            for (label, value, secret) in [
                                (username_label.as_str(), &mut self.username, false),
                                ("Pandanet password", &mut *self.password, true),
                            ] {
                                if secret && !needs_password {
                                    continue;
                                }
                                let label = ui.label(label);
                                ui.add(
                                    egui::TextEdit::singleline(value)
                                        .password(secret)
                                        .desired_width(260.),
                                )
                                .labelled_by(label.id);
                                ui.end_row();
                            }
                        });
                    ui.horizontal(|ui| {
                        if needs_password {
                            ui.checkbox(&mut self.remember, "Remember login in Keychain");
                        }
                        if ui
                            .add_enabled(
                                !self.username.trim().is_empty()
                                    && (!needs_password || !self.password.is_empty()),
                                egui::Button::new("Connect and sync"),
                            )
                            .clicked()
                        {
                            actions.command = Some((
                                state.server,
                                Command::Connect {
                                    username: self.username.clone(),
                                    password: if needs_password {
                                        std::mem::take(&mut *self.password)
                                    } else {
                                        String::new()
                                    },
                                    remember: needs_password && self.remember,
                                },
                            ));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(!state.username.is_empty(), |ui| {
                            let mut automatic = state.automatic;
                            if ui.checkbox(&mut automatic, "Automatic sync").changed() {
                                actions.command =
                                    Some((state.server, Command::SetAutomatic(automatic)));
                            }
                            let label = if needs_password {
                                "Disconnect and forget login".to_owned()
                            } else {
                                format!("Disconnect {}", state.server.label())
                            };
                            if ui.button(label).clicked() {
                                actions.command = Some((state.server, Command::Disconnect));
                            }
                        });
                    });
                    ui.small(format!("Originals: {}", folder.display()));
                    ui.separator();
                }
                if state.syncing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!(
                            "{} · {} downloaded this sync",
                            state.status, state.downloaded
                        ));
                    });
                } else if self.settings_open {
                    ui.label(&state.status);
                }
                if let Some(error) = &state.error {
                    ui.colored_label(egui::Color32::from_rgb(255, 151, 136), error);
                }
                if !state.failures.is_empty() {
                    ui.collapsing(
                        format!("{} downloads to retry", state.failures.len()),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(120.)
                                .show(ui, |ui| {
                                    for failure in &state.failures {
                                        ui.label(format!("{} — {}", failure.date, failure.message));
                                    }
                                });
                        },
                    );
                }
                ui.separator();
                if !self.settings_open
                    && has_games
                    && self.show_rank_history
                    && !self.show_play_times
                {
                    rank_chart::show(ui, states, self.filter);
                }
                let today = super::period::local_today();
                let mut statistics = self.statistics.unwrap_or_default();
                ui.horizontal_wrapped(|ui| {
                    self.game_filters(ui, states, has_games, statistics.thresholds)
                });
                ui.horizontal_wrapped(|ui| {
                    if self.statistics.is_some() {
                        self.period_controls.show(
                            ui,
                            &mut statistics.period,
                            &mut statistics.compare,
                            today,
                        );
                        if !self.show_play_times {
                            thresholds::show(
                                ui,
                                &mut self.threshold_editor,
                                &mut statistics.thresholds,
                            );
                        }
                        self.statistics = Some(statistics);
                    }
                    let label = ui.label("Search synced games");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search by date, player, or result")
                            .desired_width((ui.available_size_before_wrap().x - 12.0).max(130.0)),
                    )
                    .labelled_by(label.id);
                });
                if let Some(path) = table::show(
                    ui,
                    &states.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
                    &mut self.sort,
                    table::Filter {
                        server: self.filter,
                        search: &self.search,
                        category: self.category,
                        evidence: &self.categories,
                        thresholds: statistics.thresholds,
                        period: statistics.period.windows(today),
                        compare: statistics.compare,
                    },
                    self.show_play_times.then_some(&mut self.by_hour),
                ) {
                    actions.review = Some(path);
                    close = true;
                }
                ui.separator();
                if ui.button("Close library").clicked() {
                    close = true;
                }
            });
        self.open = open && !close;
        if !self.open {
            self.password.zeroize();
        }
        actions
    }
}
