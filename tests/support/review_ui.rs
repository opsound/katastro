//! Drive the review UI against its real background worker.
use eframe::egui;
use katastro::{
    ui,
    worker::{Client, Snapshot},
};
use std::time::{Duration, Instant};

pub struct WorkerUi {
    pub client: Client,
    pub snapshot: Snapshot,
    pub commands_sent: usize,
}
impl WorkerUi {
    pub fn new(client: Client, snapshot: Snapshot) -> Self {
        Self {
            client,
            snapshot,
            commands_sent: 0,
        }
    }
    pub fn render(&mut self, ui: &mut egui::Ui) {
        for action in ui::render(ui, &self.snapshot) {
            if let ui::Action::Review(command) = action {
                self.client.send(command).unwrap();
                self.commands_sent += 1;
            }
        }
    }
}

pub fn until(client: &Client, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut latest = None;
    loop {
        let snapshot = client.recv(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|error| panic!("waiting for review update ({error}), latest selection/visits/coverage {latest:?}"));
        assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
        if predicate(&snapshot) {
            return snapshot;
        }
        latest = snapshot.document.as_ref().map(|d| {
            (
                d.selected,
                snapshot.values.get(&d.selected).map(|v| v.visits),
                snapshot.coverage,
            )
        });
    }
}
