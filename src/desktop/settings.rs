//! Engine settings edit a local draft; the worker validates and saves it.
use super::*;
impl DesktopApp {
    pub(super) fn settings(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let mut open = self.settings_open;
        let mut save = false;
        let mut choose = None;
        egui::Window::new("KataGo engine")
            .open(&mut open)
            .resizable(true)
            .vscroll(true)
            .default_width(580.0)
            .default_height(700.0)
            .show(ui.ctx(), |ui| {
                ui.label("Analysis runs locally. Your SGFs and reviews stay on this Mac.");
                ui.add_space(12.0);
                ui.label("KataGo executable");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.executable_text).desired_width(410.0));
                    if ui.button("Choose executable").clicked() {
                        choose = Some(DialogRequest::EngineExecutable);
                    }
                });
                ui.label("Neural network model (.bin.gz)");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.model_text).desired_width(410.0));
                    if ui.button("Choose model").clicked() {
                        choose = Some(DialogRequest::EngineModel);
                    }
                });
                ui.add_space(12.0);
                ui.strong("Review depth");
                let mut limited = self.analysis_settings.max_visits.is_some();
                if ui.checkbox(&mut limited, "Limit review analysis").changed() {
                    self.analysis_settings.max_visits = limited.then_some(self.visit_limit);
                }
                ui.add_enabled_ui(limited, |ui| {
                    ui.horizontal(|ui| {
                        let label = ui.label("Visits per position");
                        if ui.add(egui::DragValue::new(&mut self.visit_limit).range(64..=1_048_576).speed(16))
                            .labelled_by(label.id).changed() {
                            self.analysis_settings.max_visits = Some(self.visit_limit);
                        }
                    });
                });
                ui.label("More visits allow a deeper search and take longer. The chart fills quickly before refinement. Unlimited analysis continues until paused.");
                ui.add_space(12.0);
                ui.strong("Compute use");
                egui::Grid::new("engine-compute").spacing([20.0,8.0]).show(ui, |ui| {
                    let label = ui.label("Parallel positions");
                    ui.add(egui::DragValue::new(&mut self.analysis_settings.parallel_positions).range(1..=32)).labelled_by(label.id);
                    ui.end_row();
                    let label = ui.label("Threads per position");
                    ui.add(egui::DragValue::new(&mut self.analysis_settings.threads_per_position).range(1..=16)).labelled_by(label.id);
                    ui.end_row();
                });
                ui.label("Parallel positions spreads work across the game. Threads per position assigns more CPU work to each search. Higher settings are not always faster.");
                ui.label(format!("Up to {} search threads; GPU batch size adjusts automatically.", self.analysis_settings.parallel_positions * self.analysis_settings.threads_per_position));
                ui.label("Training shares these compute settings and keeps its fixed grading depth.");
                ui.add_space(12.0);
                if ui.button("Restore analysis defaults").clicked() {
                    self.analysis_settings = Default::default();
                    self.visit_limit = 1024;
                }
                ui.label("Saving pauses review analysis. Choose Analyze to resume. Compatible cached results are retained.");
                ui.add_space(12.0);
                if ui.button("Save engine settings").clicked() {
                    save = true;
                }
                if let Some(diagnostic) = &self.state.diagnostic {
                    ui.collapsing("Latest engine message", |ui| { ui.label(diagnostic); });
                }
            });
        self.settings_open = open;
        if let Some(request) = choose {
            self.begin_dialog(request, frame);
        }
        if save {
            self.action(
                Action::Review(Command::Configure(EngineConfig {
                    analysis: self.analysis_settings.clone(),
                    executable: self.executable_text.trim().into(),
                    model: self.model_text.trim().into(),
                })),
                frame,
            );
            self.settings_open = false;
        }
    }
}
