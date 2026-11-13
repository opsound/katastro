#![cfg(feature = "desktop")]
#[path = "support/training.rs"]
mod support;
use egui_kittest::{Harness, kittest::Queryable};
use katastro::training::{
    Phase, Snapshot, Trainer,
    client::Command,
    view::{Action, View},
};
struct State {
    trainer: Trainer,
    review: katastro::Review,
    snapshot: Snapshot,
    view: View,
    library: katastro::pandanet::Snapshot,
}
impl State {
    fn render(&mut self, ui: &mut eframe::egui::Ui) {
        for action in self.view.render(ui, &self.snapshot, &self.library) {
            match action {
                Action::Start {
                    games,
                    account,
                    settings,
                } => self.trainer.begin(games, account, settings).unwrap(),
                Action::Command(command) => match command {
                    Command::Play(p) => self.trainer.play(p).unwrap(),
                    Command::Retry => self.trainer.retry().unwrap(),
                    Command::Reveal => self.trainer.reveal().unwrap(),
                    Command::Cancel => self.trainer.cancel(),
                },
                Action::ExploreLine(line) => {
                    let (path, index) = self.snapshot.reveal.as_ref().unwrap();
                    self.review.import(path).unwrap();
                    let anchor = self.review.document().unwrap().mainline[*index];
                    self.review.select(anchor).unwrap();
                    for point in line {
                        self.review.play(point).unwrap();
                    }
                }
                Action::Back => {}
            }
        }
        support::settle(&mut self.trainer, false, false);
        self.snapshot = self.trainer.snapshot();
    }
}
// Benefit: the complete blind attempt is playable by board input, with no review
// controls or hints to leak answers; failure offers a real retry before revealing.
#[test]
fn training_screen_plays_grades_and_retries_without_leaking_review_hints() {
    let temp = tempfile::tempdir().unwrap();
    let source = support::fixture(temp.path(), false);
    let library = katastro::pandanet::Snapshot {
        username: "me".into(),
        games: vec![katastro::pandanet::Game {
            id: "test".into(),
            date: "2026-01-01".into(),
            white: "opponent".into(),
            black: "me".into(),
            result: "W+R".into(),
            path: source,
        }],
        ..Default::default()
    };
    let mut view = View::default();
    view.show(&library);
    let state = State {
        trainer: support::trainer(temp.path()),
        review: katastro::Review::new(&temp.path().join("reviews"), &temp.path().join("cache"))
            .unwrap(),
        snapshot: Snapshot::default(),
        view,
        library,
    };
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200., 820.))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    if let Some(folder) = std::env::var_os("KATASTRO_TRAINING_PREVIEW") {
        h.render()
            .unwrap()
            .save(std::path::PathBuf::from(folder).join("setup.png"))
            .unwrap();
    }
    h.get_by_label("Start training").click();
    h.run_steps(3);
    assert_eq!(h.state().snapshot.phase, Phase::YourTurn);
    if let Some(folder) = std::env::var_os("KATASTRO_TRAINING_PREVIEW") {
        h.render()
            .unwrap()
            .save(std::path::PathBuf::from(folder).join("attempt.png"))
            .unwrap();
    }
    for hidden in [
        "AI moves",
        "Next move",
        "Score lead",
        "Winrate",
        "SECRET SOLUTION",
    ] {
        assert!(h.query_by_label(hidden).is_none(), "leaked {hidden}");
    }
    for coordinate in ["A5", "B5", "C5", "D5", "E5"] {
        h.get_by_label(&format!("Play {coordinate}")).click();
        h.run_steps(3);
    }
    assert_eq!(h.state().snapshot.phase, Phase::Failed);
    assert!(
        h.query_by_label("Total losses on your moves only:")
            .is_some()
    );
    assert!(h.state().snapshot.reveal.is_none());
    h.get_by_label("Retry without hints").click();
    h.run_steps(3);
    assert_eq!(h.state().snapshot.phase, Phase::YourTurn);
    assert_eq!(h.state().snapshot.played, 0);
    let d = h.state().snapshot.document.as_ref().unwrap();
    assert_eq!(d.board(d.selected).unwrap().move_number, 0);
    h.get_by_label("Give up and explore").click();
    h.run_steps(3);
    assert_eq!(h.state().snapshot.phase, Phase::Revealed);
    assert!(h.state().snapshot.reveal.is_some());
    h.get_by_label("Explore line 1").click();
    h.run_steps(3);
    let d = h.state().review.document().unwrap();
    let b = d.board(d.selected).unwrap();
    assert_eq!(b.move_number, 3);
    assert_eq!(
        b.stone(katastro::Point::new(3, 4)),
        Some(katastro::Color::Black)
    );
    h.get_by_label("Exercise position").click();
    h.run_steps(3);
    let d = h.state().review.document().unwrap();
    assert_eq!(d.board(d.selected).unwrap().move_number, 0);
}
