use super::{Command, Server, Snapshot};
use eframe::egui;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;
mod table;
#[derive(Default)]
pub struct View {
    pub open: bool,
    filter: Option<Server>,
    server: Server,
    settings_open: bool,
    username: String,
    password: String,
    remember: bool,
    search: String,
    sort: table::Sort,
}
pub struct Actions {
    pub server: Server,
    pub command: Option<Command>,
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
    pub fn render(&mut self, ctx: &egui::Context, states: &[(&Snapshot, &Path)]) -> Actions {
        let mut actions = Actions {
            server: self.server,
            command: None,
            review: None,
            train: false,
        };
        let mut open = self.open;
        let mut close = false;
        egui::Window::new("Game library")
            .open(&mut open)
            .default_width(1000.)
            .default_height(580.)
            .show(ctx, |ui| {
                let Some((state, folder)) = states.iter().find(|(s, _)| s.server == self.server)
                else {
                    return;
                };
                actions.server = self.server;
                ui.horizontal(|ui| {
                    ui.heading(format!(
                        "{} synced games",
                        states.iter().map(|(s, _)| s.games.len()).sum::<usize>()
                    ));
                    if ui
                        .add_enabled(
                            states.iter().any(|(s, _)| {
                                self.filter.is_none_or(|f| f == s.server) && !s.games.is_empty()
                            }),
                            egui::Button::new("Train from these games"),
                        )
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
                            actions.command = Some(Command::SetAutomatic(false));
                        }
                    } else if ui
                        .add_enabled(
                            !state.username.is_empty(),
                            egui::Button::new(format!("Sync {} now", state.server.label())),
                        )
                        .clicked()
                    {
                        actions.command = Some(Command::SyncNow);
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
                        actions.command = Some(Command::SetLimit(limit));
                    }
                    ui.small("Older downloads are kept.");
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
                    if state.server == Server::Fox {
                        ui.small("Public game records · only your username is needed.");
                    }
                    egui::Grid::new("pandanet-login")
                        .num_columns(2)
                        .show(ui, |ui| {
                            let label = ui.label(format!("{} username", state.server.label()));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.username).desired_width(260.),
                            )
                            .labelled_by(label.id);
                            ui.end_row();
                            if state.server == Server::Pandanet {
                                let label = ui.label("Pandanet password");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.password)
                                        .password(true)
                                        .desired_width(260.),
                                )
                                .labelled_by(label.id);
                                ui.end_row();
                            }
                        });
                    ui.horizontal(|ui| {
                        if state.server == Server::Pandanet {
                            ui.checkbox(&mut self.remember, "Remember login in Keychain");
                        }
                        if ui
                            .add_enabled(
                                !self.username.trim().is_empty()
                                    && (state.server == Server::Fox || !self.password.is_empty()),
                                egui::Button::new("Connect and sync"),
                            )
                            .clicked()
                        {
                            actions.command = Some(Command::Connect {
                                username: self.username.clone(),
                                password: std::mem::take(&mut self.password),
                                remember: self.remember,
                            });
                        }
                    });
                    ui.horizontal(|ui| {
                        let mut automatic = state.automatic;
                        if ui
                            .add_enabled(
                                !state.username.is_empty(),
                                egui::Checkbox::new(&mut automatic, "Automatic sync"),
                            )
                            .changed()
                        {
                            actions.command = Some(Command::SetAutomatic(automatic));
                        }
                        if ui
                            .add_enabled(
                                !state.username.is_empty(),
                                egui::Button::new(if state.server == Server::Fox {
                                    "Disconnect Fox"
                                } else {
                                    "Disconnect and forget login"
                                }),
                            )
                            .clicked()
                        {
                            actions.command = Some(Command::Disconnect);
                        }
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
                } else {
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
                let label = ui
                    .horizontal(|ui| {
                        let label = ui.label("Search synced games");
                        for (filter, text) in [
                            (None, "All servers"),
                            (Some(Server::Pandanet), "Pandanet"),
                            (Some(Server::Fox), "Fox"),
                        ] {
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
                                if let Some((other, _)) =
                                    states.iter().find(|(s, _)| Some(s.server) == filter)
                                {
                                    let settings_open = self.settings_open;
                                    self.show(other);
                                    self.settings_open = settings_open;
                                }
                            }
                        }
                        label
                    })
                    .inner;
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Date, player, or result")
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
                if let Some(path) = table::show(
                    ui,
                    &states.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
                    self.filter,
                    &self.search,
                    &mut self.sort,
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
impl Drop for View {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}
