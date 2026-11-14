#![cfg(feature = "desktop")]
#[path = "support/training.rs"]
mod support;
use egui_kittest::{Harness, kittest::Queryable};
use katastro::training::{
    Phase, Snapshot, Trainer,
    client::Command,
    view::{Action, View},
};

// Benefit: a completed grade remains usable while the last chart points arrive.
// Missing endpoints must be visibly pending, never shown as zero evaluations.
#[test]
fn report_keeps_controls_available_while_comparison_finishes() {
    use eframe::egui;
    struct ReportView {
        view: View,
        snapshot: Snapshot,
        actions: Vec<Action>,
    }
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
    for i in 0..5 {
        trainer.play(Some(katastro::Point::new(i, 4))).unwrap();
        support::settle(&mut trainer, false, true);
    }
    let mut snapshot = trainer.snapshot();
    let comparison = &mut snapshot.report.as_mut().unwrap().comparison;
    comparison.total_plies = 10;
    comparison.attempt.truncate(10);
    comparison.recorded.truncate(10);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200., 820.))
        .build_ui_state(
            |ui, s: &mut ReportView| {
                s.actions
                    .extend(s.view.render(ui, &s.snapshot, &Default::default()));
            },
            ReportView {
                view: View::default(),
                snapshot,
                actions: vec![],
            },
        );
    h.run();
    h.get_by_label("Passed");
    h.get_by_label("Finishing comparison…");
    for name in ["Training score comparison", "Training winrate comparison"] {
        h.get_by_label(name);
    }
    let plotted = |h: &Harness<'_, ReportView>| {
        let bounds = h.get_by_label("Training winrate comparison").rect();
        h.output()
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Path(p)
                    if p.points.len() >= 10
                        && p.points.iter().all(|v| bounds.contains(*v))
                        && p.stroke.color
                            == egui::epaint::ColorMode::Solid(egui::Color32::from_rgb(
                                90, 190, 156,
                            )) =>
                {
                    Some(p.points.clone())
                }
                _ => None,
            })
            .unwrap()
    };
    let before = plotted(&h);
    assert_eq!(
        before.len(),
        10,
        "missing endpoint must not be replaced by zero"
    );
    if let Some(folder) = std::env::var_os("KATASTRO_TRAINING_PREVIEW") {
        h.render()
            .unwrap()
            .save(std::path::PathBuf::from(folder).join("report-pending.png"))
            .unwrap();
    }
    h.get_by_label("Next problem").click();
    h.run();
    assert!(
        h.state()
            .actions
            .iter()
            .any(|a| matches!(a, Action::Command(Command::Next)))
    );
    let comparison = &mut h.state_mut().snapshot.report.as_mut().unwrap().comparison;
    comparison.attempt.push(katastro::training::Evaluation {
        winrate: 0.8,
        score: 8.0,
    });
    comparison.recorded.push(katastro::training::Evaluation {
        winrate: 0.4,
        score: -2.0,
    });
    h.run();
    assert!(h.query_by_label("Finishing comparison…").is_none());
    h.get_by_label("Passed");
    let after = plotted(&h);
    assert_eq!(after.len(), 11);
    assert!(
        (before[9].x - after[9].x).abs() < 0.1,
        "finishing the endpoint must not rescale the move axis"
    );
}
struct State {
    trainer: Trainer,
    review: katastro::Review,
    snapshot: Snapshot,
    view: View,
    library: katastro::pandanet::Snapshot,
    settle: bool,
    good: bool,
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
                    Command::Next => self.trainer.next_problem().unwrap(),
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
            support::settle(&mut self.trainer, false, self.good);
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
        good: false,
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
        good: true,
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

// Benefit: every completed attempt has a visible way forward, including while
// exploring. Regression: the button is missing or repeats the same problem.
#[test]
fn next_problem_advances_after_pass_fail_and_exploration() {
    for outcome in [Phase::Passed, Phase::Failed, Phase::Revealed] {
        let temp = tempfile::tempdir().unwrap();
        let source = support::fixture(temp.path(), false);
        let other = temp.path().join("other.sgf");
        std::fs::write(
            &other,
            std::fs::read_to_string(&source)
                .unwrap()
                .replace("KM[6.5]", "KM[7.5]"),
        )
        .unwrap();
        let mut trainer = support::trainer(temp.path());
        trainer
            .begin(
                vec![source, other],
                "me".into(),
                katastro::training::Settings {
                    moves: 1,
                    include_completed: true,
                    ..Default::default()
                },
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
            settle: true,
            good: outcome == Phase::Passed,
        };
        let mut h = Harness::builder()
            .with_size(eframe::egui::vec2(1200., 820.))
            .build_ui_state(|ui, state: &mut State| state.render(ui), state);
        if outcome == Phase::Revealed {
            h.get_by_label("Give up and explore").click();
            h.run_steps(3);
            h.get_by_label("Explore line 1").click();
        } else {
            h.get_by_label("Play A5").click();
        }
        h.run_steps(3);
        assert_eq!(h.state().snapshot.phase, outcome);
        if outcome == Phase::Failed {
            assert!(h.query_by_label("Retry without hints").is_some());
        }
        h.get_by_label("Next problem").click();
        h.run_steps(3);
        assert_eq!(h.state().snapshot.phase, Phase::YourTurn);
        assert_eq!(h.state().snapshot.document.as_ref().unwrap().komi, 7.5);
        assert_eq!(h.state().snapshot.played, 0);
        assert!(h.state().snapshot.report.is_none() && h.state().snapshot.reveal.is_none());
        h.get_by_label("Play B5").click();
        h.run_steps(3);
        assert_eq!(h.state().snapshot.played, 1);
    }
}

// Benefit: after real board play, two report charts visibly overlay the attempt
// and recorded continuation. They are absent during blind play and after retry.
#[test]
fn report_draws_both_evaluation_overlays_and_retry_hides_them() {
    use eframe::egui::{Color32, Shape, epaint::ColorMode};
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
        settle: true,
        good: true,
    };
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200., 820.))
        .build_ui_state(|ui, s: &mut State| s.render(ui), state);
    assert!(h.query_by_label("Training winrate comparison").is_none());
    for point in ["A5", "B5", "C5", "D5", "E5"] {
        h.get_by_label(&format!("Play {point}")).click();
        h.run_steps(3);
    }
    assert_eq!(h.state().snapshot.phase, Phase::Passed);
    for label in ["Training score comparison", "Training winrate comparison"] {
        let plot = h
            .query_by_label(label)
            .expect("report must show both comparison charts")
            .rect();
        let paths: Vec<_> = h
            .output()
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::Path(p)
                    if p.points.len() == 11 && p.points.iter().all(|v| plot.contains(*v)) =>
                {
                    Some(p)
                }
                _ => None,
            })
            .collect();
        let recorded = paths
            .iter()
            .find(|p| p.stroke.color == ColorMode::Solid(Color32::from_rgb(126, 167, 244)))
            .expect("recorded line is visible");
        let attempt = paths
            .iter()
            .find(|p| p.stroke.color == ColorMode::Solid(Color32::from_rgb(90, 190, 156)))
            .expect("attempt overlay is visible");
        assert!(
            (recorded.points[0].y - attempt.points[0].y).abs() < 0.1,
            "same starting evaluation"
        );
        assert!(
            recorded.points[10].y > attempt.points[10].y + 5.0,
            "recorded loss is visible below the clean attempt"
        );
        assert!((attempt.points[0].y - attempt.points[10].y).abs() < 0.1);
    }
    if let Some(folder) = std::env::var_os("KATASTRO_TRAINING_PREVIEW") {
        h.render()
            .unwrap()
            .save(std::path::PathBuf::from(folder).join("report.png"))
            .unwrap();
    }
    // A fresh failed attempt also shows the chart, but blind retry removes it.
    h.state_mut()
        .trainer
        .begin(
            vec![support::fixture(temp.path(), false)],
            "me".into(),
            katastro::training::Settings {
                include_completed: true,
                ..Default::default()
            },
        )
        .unwrap();
    h.state_mut().good = false;
    h.run_steps(3);
    for point in ["A4", "B4", "C4", "D4", "E4"] {
        h.get_by_label(&format!("Play {point}")).click();
        h.run_steps(3);
    }
    assert_eq!(h.state().snapshot.phase, Phase::Failed);
    assert!(h.query_by_label("Training winrate comparison").is_some());
    h.get_by_label("Retry without hints").click();
    h.run_steps(3);
    assert!(h.query_by_label("Training winrate comparison").is_none());
    h.get_by_label("Play A5").click();
    h.run_steps(3);
    assert_eq!(h.state().snapshot.played, 1);
}
