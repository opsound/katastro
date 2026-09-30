#![cfg(feature = "desktop")]
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{
    Color, Point, Review,
    engine::EngineConfig,
    ui,
    worker::{Command, Snapshot},
};
use std::path::Path;
struct State {
    review: Review,
    snapshot: Snapshot,
}
fn state(root: &Path) -> State {
    let source = root.join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc];W[gg];B[cg])").unwrap();
    let mut review = Review::new(&root.join("reviews"), &root.join("cache")).unwrap();
    review.import(&source).unwrap();
    let mut state = State {
        review,
        snapshot: Snapshot::empty(EngineConfig {
            executable: Default::default(),
            model: Default::default(),
        }),
    };
    state.sync();
    state
}
impl State {
    fn sync(&mut self) {
        self.snapshot.document = self.review.document().cloned();
        self.snapshot.board = self.review.document().map(|d| d.board(d.selected).unwrap());
    }
    fn render(&mut self, ui: &mut eframe::egui::Ui) {
        for action in ui::render(ui, &self.snapshot) {
            let ui::Action::Review(command) = action else {
                continue;
            };
            match command {
                Command::Play(point) => {
                    self.review.play(point).unwrap();
                }
                Command::Select(id) => {
                    self.review.select(id).unwrap();
                }
                Command::First => {
                    self.review.select(0).unwrap();
                }
                Command::Step(false) => {
                    let parent = self.review.document().unwrap().nodes
                        [self.review.document().unwrap().selected]
                        .parent;
                    if let Some(id) = parent {
                        self.review.select(id).unwrap();
                    }
                }
                Command::Step(true) => {
                    let child = self.review.document().unwrap().nodes
                        [self.review.document().unwrap().selected]
                        .children
                        .first()
                        .copied();
                    if let Some(id) = child {
                        self.review.select(id).unwrap();
                    }
                }
                _ => {}
            }
        }
        self.sync();
    }
}
#[test]
fn actual_board_and_tree_input_creates_and_selects_persistent_variations() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 820.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state(temp.path()));
    h.get_by_label("Next move").click();
    h.run();
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 1);
    h.get_by_label("Play D6").click();
    h.run();
    let branch = h.state().snapshot.document.as_ref().unwrap().selected;
    assert_eq!(
        h.state()
            .snapshot
            .board
            .as_ref()
            .unwrap()
            .stone(Point::new(3, 3)),
        Some(Color::White)
    );
    assert_eq!(
        h.state().snapshot.document.as_ref().unwrap().mainline.len(),
        4
    );
    h.get_by_label("Previous move").click();
    h.run();
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 1);
    let branch_bounds = h.get_by_label(&format!("Tree node {branch}")).rect();
    let status_bounds = h.get_by_label("Open an SGF to begin").rect();
    assert!(
        branch_bounds.max.y <= status_bounds.min.y - 9.0,
        "the first variation must fit completely above the status bar: {branch_bounds:?} {status_bounds:?}"
    );
    h.get_by_label(&format!("Tree node {branch}")).click();
    h.run();
    assert_eq!(
        h.state().snapshot.document.as_ref().unwrap().selected,
        branch
    );
    drop(h);
    let mut reopened = state(temp.path());
    reopened
        .review
        .import(&temp.path().join("game.sgf"))
        .unwrap();
    assert_eq!(reopened.review.document().unwrap().nodes.len(), 5);
}
#[test]
fn keyboard_navigation_and_pass_work_with_loaded_review() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 820.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state(temp.path()));
    h.key_press(eframe::egui::Key::ArrowRight);
    h.run();
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 1);
    h.get_by_label("Pass").click();
    h.run();
    assert!(
        h.state()
            .snapshot
            .document
            .as_ref()
            .unwrap()
            .nodes
            .last()
            .unwrap()
            .played
            .unwrap()
            .point
            .is_none()
    );
}
#[test]
fn original_game_chart_keeps_gaps_zero_values_and_its_line_when_a_branch_is_selected() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state.snapshot.values.insert(
        1,
        katastro::Analysis {
            visits: 1,
            winrate: 0.0,
            score_lead: 0.0,
        },
    );
    assert_eq!(
        ui::chart_points(&state.snapshot, true),
        vec![None, Some([1.0, 0.0]), None, None]
    );
    state.review.select(1).unwrap();
    let branch = state.review.play(Some(Point::new(3, 3))).unwrap();
    state.sync();
    state.snapshot.values.insert(
        branch,
        katastro::Analysis {
            visits: 64,
            winrate: 0.9,
            score_lead: 99.0,
        },
    );
    assert_eq!(
        ui::chart_points(&state.snapshot, false),
        vec![None, Some([1.0, 0.0]), None, None]
    );
}
