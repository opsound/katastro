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
fn imported_state(root: &Path, sgf: &str) -> State {
    let mut result = state(root);
    std::fs::write(root.join("game.sgf"), sgf).unwrap();
    result.review.import(&root.join("game.sgf")).unwrap();
    result.sync();
    result
}
impl State {
    fn sync(&mut self) {
        self.snapshot.document = self.review.document().cloned();
        self.snapshot.board = self.review.document().map(|d| d.board(d.selected).unwrap());
    }
    fn render(&mut self, ui: &mut eframe::egui::Ui) {
        ui.ctx().global_style_mut(|style| {
            style.spacing.item_spacing = eframe::egui::vec2(10.0, 8.0);
            style.spacing.button_padding = eframe::egui::vec2(12.0, 7.0);
        });
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
            suggestions: vec![],
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
            suggestions: vec![],
        },
    );
    assert_eq!(
        ui::chart_points(&state.snapshot, false),
        vec![None, Some([1.0, 0.0]), None, None]
    );
}

#[test]
fn horizontal_navigation_keeps_the_selected_timeline_node_in_view() {
    let temp = tempfile::TempDir::new().unwrap();
    let sgf = format!("(;SZ[9]{})", ";B[];W[]".repeat(60));
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 820.0))
        .build_ui_state(
            |ui, state: &mut State| state.render(ui),
            imported_state(temp.path(), &sgf),
        );
    for _ in 0..80 {
        h.key_press(eframe::egui::Key::ArrowRight);
        h.run();
    }
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 80);
    let selected = h
        .query_by_label("Tree node 80")
        .expect("the current move scrolled out of the timeline")
        .rect();
    assert!(
        h.get_by_label("Timeline viewport")
            .rect()
            .contains_rect(selected.expand(2.0))
    );
    // A tree click must not steal the navigation keys from the review.
    h.get_by_label("Tree node 80").click();
    h.run();
    for _ in 0..80 {
        h.key_press(eframe::egui::Key::ArrowLeft);
        h.run();
    }
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 0);
    let root = h.get_by_label("Tree node 0").rect();
    assert!(
        h.get_by_label("Timeline viewport")
            .rect()
            .contains_rect(root.expand(2.0))
    );
}

#[test]
fn up_down_selects_variations_in_the_same_column_and_survives_reopen() {
    let temp = tempfile::TempDir::new().unwrap();
    let sgf = "(;SZ[9];B[cc](;W[gg];B[cg];W[gc])(;W[dd](;B[ee];W[ff])(;B[ed];W[fd]))(;W[ce]))";
    let mut state = imported_state(temp.path(), sgf);
    state.review.select(3).unwrap();
    state.sync();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 820.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    h.key_press(eframe::egui::Key::ArrowDown);
    h.run();
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 6);
    assert_eq!(
        h.state()
            .snapshot
            .board
            .as_ref()
            .unwrap()
            .stone(Point::new(4, 4)),
        Some(Color::Black)
    );
    h.key_press(eframe::egui::Key::ArrowDown);
    h.run();
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 8);
    h.key_press(eframe::egui::Key::ArrowDown);
    h.run();
    assert_eq!(
        h.state().snapshot.document.as_ref().unwrap().selected,
        8,
        "bottom boundary must not wrap"
    );
    h.key_press(eframe::egui::Key::ArrowUp);
    h.run();
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 6);
    h.get_by_label("Tree node 2").click();
    h.run();
    h.key_press(eframe::egui::Key::ArrowDown);
    h.run();
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 5);
    h.key_press(eframe::egui::Key::ArrowDown);
    h.run();
    assert_eq!(
        h.state().snapshot.document.as_ref().unwrap().selected,
        10,
        "skip rows with no node in this column"
    );
    drop(h);
    let reopened = imported_state(temp.path(), sgf);
    assert_eq!(reopened.snapshot.document.as_ref().unwrap().selected, 10);
    assert!(
        reopened
            .snapshot
            .document
            .as_ref()
            .unwrap()
            .layout()
            .iter()
            .filter(|p| p.row == 0)
            .all(|p| reopened
                .snapshot
                .document
                .as_ref()
                .unwrap()
                .mainline
                .contains(&p.id))
    );
}

#[test]
fn analysis_column_stays_fixed_when_playing_and_refining_a_variation() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 64,
            score_lead: 2.5,
            winrate: 0.55,
            suggestions: vec![],
        },
    );
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1000.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let score = h.get_by_label("SCORE LEAD · POINTS").rect();
    let winrate = h.get_by_label("WINRATE · BLACK %").rect();
    h.get_by_label("Play D6").click();
    h.run();
    let selected = h.state().snapshot.document.as_ref().unwrap().selected;
    assert_eq!(
        h.get_by_label("SCORE LEAD · POINTS").rect(),
        score,
        "awaiting analysis moved the score chart"
    );
    assert_eq!(h.get_by_label("WINRATE · BLACK %").rect(), winrate);
    for visits in [1, 64, 256] {
        h.state_mut().snapshot.values.insert(
            selected,
            katastro::Analysis {
                visits,
                score_lead: -12.5,
                winrate: 0.0,
                suggestions: suggestions(),
            },
        );
        h.run();
        assert_eq!(h.get_by_label("SCORE LEAD · POINTS").rect(), score);
        assert_eq!(h.get_by_label("WINRATE · BLACK %").rect(), winrate);
    }
}

fn suggestions() -> Vec<katastro::SuggestedMove> {
    [Point::new(3, 3), Point::new(4, 4)]
        .into_iter()
        .map(|point| katastro::SuggestedMove {
            point: Some(point),
            visits: 32,
            winrate: Some(0.6),
            score_lead: Some(2.5),
        })
        .collect()
}
fn has_circle(
    h: &Harness<'_, State>,
    rect: eframe::egui::Rect,
    predicate: impl Fn(&eframe::egui::epaint::CircleShape) -> bool,
) -> bool {
    h.output().shapes.iter().any(|shape| matches!(&shape.shape, eframe::egui::Shape::Circle(circle) if rect.contains(circle.center) && predicate(circle)))
}
#[test]
fn suggestions_are_ranked_blue_clickable_and_specific_to_the_position() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 256,
            winrate: 0.6,
            score_lead: 2.5,
            suggestions: suggestions(),
        },
    );
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    if let Some(path) = std::env::var_os("KATASTRO_BOARD_PREVIEW") {
        h.run();
        h.render().unwrap().save(path).unwrap();
    }
    let best = h.get_by_label("AI suggestion 1: D6").rect();
    assert!(
        has_circle(&h, best, |c| c.fill.b() > c.fill.r()
            && c.fill.b() > c.fill.g()),
        "the top candidate is not blue"
    );
    assert!(h.query_by_label("AI suggestion 2: E5").is_some());
    h.get_by_label("Play D6").click();
    h.run();
    assert_eq!(
        h.state()
            .snapshot
            .board
            .as_ref()
            .unwrap()
            .stone(Point::new(3, 3)),
        Some(Color::Black)
    );
    assert!(
        h.query_by_label("AI suggestion 1: D6").is_none(),
        "suggestions from the parent leaked into the new position"
    );
    h.get_by_label("Previous move").click();
    h.run();
    assert!(h.query_by_label("AI suggestion 1: D6").is_some());
    h.get_by_label("1  D6").click();
    h.run();
    assert_eq!(
        h.state().snapshot.document.as_ref().unwrap().nodes.len(),
        5,
        "playing a suggestion should reuse the recorded variation"
    );
}
#[test]
fn next_move_is_a_hollow_dotted_ring_with_the_recorded_stone_color() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state(temp.path()));
    for (label, color) in [
        ("Next recorded move: Black C7", eframe::egui::Color32::BLACK),
        ("Next recorded move: White G3", eframe::egui::Color32::WHITE),
    ] {
        let ring = h.get_by_label(label).rect();
        let arcs = h.output().shapes.iter().filter(|s|matches!(&s.shape,eframe::egui::Shape::Path(p) if !p.closed && p.fill == eframe::egui::Color32::TRANSPARENT && p.stroke.color == eframe::egui::epaint::ColorMode::Solid(color) && p.points.iter().all(|p|ring.contains(*p)))).count();
        assert!(
            arcs >= 12,
            "next-move ring must have distinct unfilled dots/arcs"
        );
        assert!(!has_circle(&h, ring, |c| c.center.distance(ring.center())
            < 0.1
            && c.radius > ring.width() * 0.3
            && c.fill != eframe::egui::Color32::TRANSPARENT));
        h.key_press(eframe::egui::Key::ArrowRight);
        h.run();
    }
    h.key_press(eframe::egui::Key::ArrowRight);
    h.run();
    assert!(
        h.query_by_label("Next recorded move: Black C3").is_none(),
        "the end of the game has no next move"
    );
    h.get_by_label("Tree node 1").click();
    h.run();
    h.get_by_label("Play D6").click();
    h.run();
    h.get_by_label("Previous move").click();
    h.run();
    assert!(
        h.query_by_label("Next recorded move: White G3").is_some(),
        "adding a variation replaced the original next move"
    );
}
#[test]
fn player_colors_depth_and_navigation_legend_are_visible_without_arrow_glyphs() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = imported_state(temp.path(), "(;SZ[9]PB[Alice]PW[Bob];B[cc])");
    state.snapshot.running = true;
    state.snapshot.analysis_target = Some(1024);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let logo = h.get_by_label("Katastro emblem").rect();
    assert!(
        has_circle(&h, logo, |c| c.fill.g() > c.fill.r() && c.radius > 2.0),
        "the emblem still depends on an unsupported glyph"
    );
    for (label, white) in [("Black player: Alice", false), ("White player: Bob", true)] {
        let rect = h.get_by_label(label).rect();
        assert!(
            has_circle(&h, rect, |c| c.fill.a() == 255
                && (c.fill.r() > 200) == white
                && c.radius >= 5.0),
            "the player lacks a colored stone"
        );
    }
    assert!(
        h.query_by_label("Refining: 1024 visits per position")
            .is_some()
    );
    let arrows = h.get_by_label("Left / Right navigation arrows").rect();
    let lines = h.output().shapes.iter().filter(|s|matches!(&s.shape,eframe::egui::Shape::LineSegment { points, stroke } if stroke.color.a()>0 && points.iter().all(|p|arrows.contains(*p)))).count();
    assert!(
        lines >= 6,
        "arrow stems and heads must be painted, independent of fonts"
    );
    assert!(h.query_by_label("Left / Right: navigate moves").is_some());
    h.state_mut().snapshot.running = false;
    h.run();
    assert!(
        h.query_by_label("Paused: target 1024 visits per position")
            .is_some()
    );
}

#[test]
fn both_charts_fit_at_the_default_window_size_with_suggestions() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 256,
            winrate: 0.6,
            score_lead: 2.5,
            suggestions: suggestions(),
        },
    );
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 860.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let tree = h.get_by_label("Timeline viewport").rect();
    for label in ["Score lead chart", "Winrate chart"] {
        let chart = h
            .query_by_label(label)
            .expect("a chart is completely hidden by the suggestions list")
            .rect();
        assert!(
            chart.max.y < tree.min.y - 20.0,
            "{label} is cut off at the default window height: {chart:?} {tree:?}"
        );
    }
}
