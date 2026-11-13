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
    settle: bool,
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
        if self.settle {
            support::settle(&mut self.trainer, false, false);
        }
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
        settle: true,
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

// Benefit: the board stays visually stable while the opponent thinks, without
// accepting an extra move. The next click works as soon as its reply arrives.
#[test]
fn waiting_keeps_the_board_bright_and_still_without_allowing_extra_moves() {
    use eframe::egui::{Color32, Shape, accesskit::Role};
    let temp = tempfile::tempdir().unwrap();
    let mut trainer = support::trainer(temp.path());
    trainer
        .begin(
            vec![support::fixture(temp.path(), false)],
            "me".into(),
            Default::default(),
        )
        .unwrap();
    support::settle(&mut trainer, false, true);
    let state = State {
        snapshot: trainer.snapshot(),
        trainer,
        review: katastro::Review::new(&temp.path().join("reviews"), &temp.path().join("cache"))
            .unwrap(),
        view: View::default(),
        library: Default::default(),
        settle: false,
    };
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200., 820.))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let wood = Color32::from_rgb(221, 185, 130);
    let board_rect = h
        .output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Rect(r) if r.fill == wood => Some(r.rect),
            _ => None,
        })
        .unwrap();
    h.get_by_label("Play A5").click();
    h.run_steps(3);
    assert_eq!(h.state().snapshot.phase, Phase::Opponent);
    let fill = h.output().shapes.iter().find_map(|s| match &s.shape {
        Shape::Rect(r) if r.rect == board_rect => Some(r.fill),
        _ => None,
    });
    assert_eq!(fill, Some(wood), "thinking must not dim or move the board");
    assert!(
        h.query_by_role(Role::ProgressIndicator).is_none(),
        "no per-move spinner"
    );
    if let Some(folder) = std::env::var_os("KATASTRO_TRAINING_PREVIEW") {
        h.render()
            .unwrap()
            .save(std::path::PathBuf::from(folder).join("thinking.png"))
            .unwrap();
    }
    h.get_by_label("Play B5").click();
    h.run_steps(3);
    assert_eq!(h.state().snapshot.board.as_ref().unwrap().move_number, 1);
    let human = h.state_mut().trainer.next_request().unwrap().unwrap();
    h.state_mut()
        .trainer
        .accept(&support::reply(&human, false, true), 0.5)
        .unwrap();
    h.run_steps(3);
    h.get_by_label("Play B5").click();
    h.run_steps(3);
    assert_eq!(h.state().snapshot.played, 2);
    assert_eq!(
        h.state()
            .snapshot
            .board
            .as_ref()
            .unwrap()
            .stone(katastro::Point::new(1, 4)),
        Some(katastro::Color::Black)
    );
}
