use super::{Command, Snapshot};
use eframe::egui;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;
mod table;
#[derive(Default)]
pub struct View {
    pub open: bool,
    settings_open: bool,
    username: String,
    password: String,
    remember: bool,
    search: String,
    sort: table::Sort,
}
pub struct Actions {
    pub command: Option<Command>,
    pub review: Option<PathBuf>,
}
fn limit_label(limit: Option<usize>) -> String {
    limit.map_or_else(|| "All games".into(), |n| format!("Latest {n} games"))
}
impl View {
    pub fn show(&mut self, state: &Snapshot) {
        self.username = state.username.clone();
        self.remember = state.remember;
        self.settings_open = true;
        self.open = true;
    }
    pub fn show_library(&mut self, state: &Snapshot) {
        self.show(state);
        self.settings_open = false;
    }
    pub fn render(&mut self, ctx: &egui::Context, state: &Snapshot, folder: &Path) -> Actions {
        let mut actions = Actions {
            command: None,
            review: None,
        };
        let mut open = self.open;
        let mut close = false;
        egui::Window::new("Game library")
            .open(&mut open)
            .default_width(1000.)
            .default_height(580.)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(format!("{} synced games", state.games.len()));
                    if ui.button("Sync settings").clicked() {
                        self.settings_open = !self.settings_open;
                    }
                    if state.syncing {
                        if ui.button("Pause sync").clicked() {
                            actions.command = Some(Command::SetAutomatic(false));
                        }
                    } else if ui
                        .add_enabled(!state.username.is_empty(), egui::Button::new("Sync now"))
                        .clicked()
                    {
                        actions.command = Some(Command::SyncNow);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Sync history");
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
                    ui.label("Pandanet · checks every 15 minutes while Katastro is open.");
                    egui::Grid::new("pandanet-login")
                        .num_columns(2)
                        .show(ui, |ui| {
                            let label = ui.label("Pandanet username");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.username).desired_width(260.),
                            )
                            .labelled_by(label.id);
                            ui.end_row();
                            let label = ui.label("Pandanet password");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.password)
                                    .password(true)
                                    .desired_width(260.),
                            )
                            .labelled_by(label.id);
                            ui.end_row();
                        });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.remember, "Remember login in Keychain");
                        if ui
                            .add_enabled(
                                !self.username.trim().is_empty() && !self.password.is_empty(),
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
                                egui::Button::new("Disconnect and forget login"),
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
                let label = ui.label("Search synced games");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Date, player, or result")
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
                if let Some(path) = table::show(ui, state, &self.search, &mut self.sort) {
                    actions.review = Some(path);
                    close = true;
                }
                ui.separator();
                if ui.button("Close Pandanet").clicked() {
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
