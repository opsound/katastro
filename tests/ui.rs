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

const CHART_VARIATION: eframe::egui::Color32 = eframe::egui::Color32::from_rgb(255, 195, 105);
fn game_chart_color(score: bool) -> eframe::egui::Color32 {
    if score {
        eframe::egui::Color32::from_rgb(95, 206, 175)
    } else {
        eframe::egui::Color32::from_rgb(126, 167, 244)
    }
}
fn chart_label(score: bool) -> &'static str {
    if score {
        "Score lead chart"
    } else {
        "Winrate chart"
    }
}
fn variation_state(root: &Path) -> State {
    let mut state = imported_state(root, "(;SZ[9];B[cc];W[gg];B[cg];W[gc])");
    state.review.select(1).unwrap();
    for point in [Point::new(3, 3), Point::new(4, 4), Point::new(5, 5)] {
        state.review.play(Some(point)).unwrap();
    }
    state.review.select(5).unwrap();
    state.sync();
    for (id, score_lead, winrate) in [
        (0, 0.0, 0.2),
        (1, 1.0, 0.3),
        (2, 2.0, 0.4),
        (3, 3.0, 0.5),
        (4, 4.0, 0.6),
        (5, -1.0, 0.25),
        (6, -2.0, 0.15),
        (7, -3.0, 0.05),
    ] {
        state.snapshot.values.insert(
            id,
            katastro::Analysis {
                visits: 64,
                score_lead,
                winrate,
                suggestions: vec![],
            },
        );
    }
    state
}
fn chart_lines(
    h: &Harness<'_, State>,
    score: bool,
    color: eframe::egui::Color32,
) -> Vec<Vec<eframe::egui::Pos2>> {
    let bounds = h.get_by_label(chart_label(score)).rect();
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            eframe::egui::Shape::Path(path)
                if !path.closed
                    && path.stroke.color == eframe::egui::epaint::ColorMode::Solid(color)
                    && path.points.iter().all(|point| bounds.contains(*point)) =>
            {
                Some(path.points.clone())
            }
            _ => None,
        })
        .collect()
}
fn chart_position(
    prefix: &[eframe::egui::Pos2],
    score: bool,
    x: f32,
    y: f32,
) -> eframe::egui::Pos2 {
    // The fixture's shared positions are (0,0),(1,1) points or (0,20),(1,30) percent.
    let (baseline, step) = if score { (0.0, 1.0) } else { (20.0, 10.0) };
    eframe::egui::pos2(
        prefix[0].x + x * (prefix[1].x - prefix[0].x),
        prefix[0].y + (y - baseline) / step * (prefix[1].y - prefix[0].y),
    )
}
fn assert_chart_line(
    h: &Harness<'_, State>,
    score: bool,
    color: eframe::egui::Color32,
    expected: &[[f32; 2]],
) {
    let prefix = chart_lines(h, score, game_chart_color(score));
    assert!(!prefix.is_empty(), "shared game prefix disappeared");
    let lines = chart_lines(h, score, color);
    assert_eq!(
        lines.len(),
        1,
        "missing or extra curve in {}: {lines:?}",
        chart_label(score)
    );
    assert_eq!(
        lines[0].len(),
        expected.len(),
        "wrong continuation in {}",
        chart_label(score)
    );
    for (actual, [x, y]) in lines[0].iter().zip(expected) {
        let expected = chart_position(&prefix[0], score, *x, *y);
        assert!(
            actual.distance(expected) < 0.1,
            "wrong assessment or move index: {actual:?} versus {expected:?}"
        );
    }
}
#[test]
fn variation_charts_dim_only_the_original_future_and_highlight_the_saved_continuation() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(
            |ui, state: &mut State| state.render(ui),
            variation_state(temp.path()),
        );
    for score in [true, false] {
        assert_eq!(
            chart_lines(&h, score, game_chart_color(score))[0].len(),
            2,
            "the original future is still highlighted after the branch point"
        );
        assert_chart_line(
            &h,
            score,
            game_chart_color(score).gamma_multiply(0.3),
            if score {
                &[[1.0, 1.0], [2.0, 2.0], [3.0, 3.0], [4.0, 4.0]]
            } else {
                &[[1.0, 30.0], [2.0, 40.0], [3.0, 50.0], [4.0, 60.0]]
            },
        );
        assert_chart_line(
            &h,
            score,
            CHART_VARIATION,
            if score {
                &[[1.0, 1.0], [2.0, -1.0], [3.0, -2.0], [4.0, -3.0]]
            } else {
                &[[1.0, 30.0], [2.0, 25.0], [3.0, 15.0], [4.0, 5.0]]
            },
        );
        let prefix = chart_lines(&h, score, game_chart_color(score));
        let selected = chart_position(&prefix[0], score, 2.0, if score { -1.0 } else { 25.0 });
        assert!(
            has_circle(&h, h.get_by_label(chart_label(score)).rect(), |c| c.fill
                == eframe::egui::Color32::WHITE
                && c.radius == 4.0
                && c.center.distance(selected) < 0.1),
            "selected variation has no marker on its own assessment"
        );
    }
    if let Some(path) = std::env::var_os("KATASTRO_CHART_PREVIEW") {
        h.run();
        h.render().unwrap().save(path).unwrap();
    }
}
#[test]
fn variation_charts_follow_nested_selection_and_restore_after_reopen() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = variation_state(temp.path());
    state.review.select(5).unwrap();
    state.review.play(Some(Point::new(4, 3))).unwrap();
    state.review.play(Some(Point::new(5, 3))).unwrap();
    state.review.select(8).unwrap();
    state.snapshot.values.insert(
        8,
        katastro::Analysis {
            visits: 64,
            score_lead: -4.0,
            winrate: 0.1,
            suggestions: vec![],
        },
    );
    state.snapshot.values.insert(
        9,
        katastro::Analysis {
            visits: 64,
            score_lead: -5.0,
            winrate: 0.0,
            suggestions: vec![],
        },
    );
    let values = state.snapshot.values.clone();
    drop(state);
    let mut reopened = imported_state(temp.path(), "(;SZ[9];B[cc];W[gg];B[cg];W[gc])");
    reopened.snapshot.values = values;
    assert_eq!(reopened.review.document().unwrap().selected, 8);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), reopened);
    for score in [true, false] {
        assert_chart_line(
            &h,
            score,
            CHART_VARIATION,
            if score {
                &[[1.0, 1.0], [2.0, -1.0], [3.0, -4.0], [4.0, -5.0]]
            } else {
                &[[1.0, 30.0], [2.0, 25.0], [3.0, 10.0], [4.0, 0.0]]
            },
        );
    }
    h.get_by_label("Tree node 6").click();
    h.run();
    assert_chart_line(
        &h,
        true,
        CHART_VARIATION,
        &[[1.0, 1.0], [2.0, -1.0], [3.0, -2.0], [4.0, -3.0]],
    );
    h.get_by_label("Tree node 3").click();
    h.run();
    for score in [true, false] {
        assert!(chart_lines(&h, score, CHART_VARIATION).is_empty());
        assert!(chart_lines(&h, score, game_chart_color(score).gamma_multiply(0.3)).is_empty());
        assert_eq!(
            chart_lines(&h, score, game_chart_color(score))[0].len(),
            5,
            "returning to the played game must restore the full original curve"
        );
    }
}
#[test]
fn variation_chart_gaps_remain_disconnected_and_zero_results_refine_in_place() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = variation_state(temp.path());
    state.snapshot.values.remove(&6);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    for score in [true, false] {
        assert_chart_line(
            &h,
            score,
            CHART_VARIATION,
            if score {
                &[[1.0, 1.0], [2.0, -1.0]]
            } else {
                &[[1.0, 30.0], [2.0, 25.0]]
            },
        );
        let prefix = chart_lines(&h, score, game_chart_color(score));
        let end = chart_position(&prefix[0], score, 4.0, if score { -3.0 } else { 5.0 });
        assert!(
            has_circle(&h, h.get_by_label(chart_label(score)).rect(), |c| c.fill
                == CHART_VARIATION
                && c.center.distance(end) < 0.1),
            "an isolated evaluated branch position was hidden"
        );
    }
    h.state_mut().snapshot.values.insert(
        6,
        katastro::Analysis {
            visits: 256,
            score_lead: 0.0,
            winrate: 0.0,
            suggestions: vec![],
        },
    );
    h.run();
    for score in [true, false] {
        assert_chart_line(
            &h,
            score,
            CHART_VARIATION,
            if score {
                &[[1.0, 1.0], [2.0, -1.0], [3.0, 0.0], [4.0, -3.0]]
            } else {
                &[[1.0, 30.0], [2.0, 25.0], [3.0, 0.0], [4.0, 5.0]]
            },
        );
    }
}
fn click_at(h: &mut Harness<'_, State>, pos: eframe::egui::Pos2) {
    h.event(eframe::egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(eframe::egui::Event::PointerButton {
            pos,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: eframe::egui::Modifiers::NONE,
        });
    }
    h.run();
}
#[test]
fn clicking_highlighted_and_dimmed_chart_curves_selects_the_corresponding_position() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(
            |ui, state: &mut State| state.render(ui),
            variation_state(temp.path()),
        );
    for score in [true, false] {
        let prefix = chart_lines(&h, score, game_chart_color(score));
        let variation = chart_position(&prefix[0], score, 3.0, if score { -2.0 } else { 15.0 });
        click_at(&mut h, variation);
        assert_eq!(
            h.state().review.document().unwrap().selected,
            6,
            "clicking the highlighted assessment selected the original game instead"
        );
        let prefix = chart_lines(&h, score, game_chart_color(score));
        let original = chart_position(&prefix[0], score, 3.0, if score { 3.0 } else { 50.0 });
        click_at(&mut h, original);
        assert_eq!(h.state().review.document().unwrap().selected, 3);
        h.get_by_label("Tree node 5").click();
        h.run();
    }
}
#[test]
fn variation_charts_extend_through_a_branch_longer_than_the_original_game() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = variation_state(temp.path());
    state.review.select(7).unwrap();
    for score_lead in [-4.0, -5.0, -6.0] {
        let id = state.review.play(None).unwrap();
        state.snapshot.values.insert(
            id,
            katastro::Analysis {
                visits: 64,
                score_lead,
                winrate: 0.0,
                suggestions: vec![],
            },
        );
    }
    state.review.select(5).unwrap();
    state.sync();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    assert_chart_line(
        &h,
        true,
        CHART_VARIATION,
        &[
            [1.0, 1.0],
            [2.0, -1.0],
            [3.0, -2.0],
            [4.0, -3.0],
            [5.0, -4.0],
            [6.0, -5.0],
            [7.0, -6.0],
        ],
    );
    let prefix = chart_lines(&h, true, game_chart_color(true));
    let end = chart_position(&prefix[0], true, 7.0, -6.0);
    assert!(
        h.get_by_label(chart_label(true)).rect().contains(end),
        "branch assessment was clipped to original length"
    );
    click_at(&mut h, end);
    assert_eq!(
        h.state().review.document().unwrap().selected,
        10,
        "the chart cannot navigate beyond the original game's final move"
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

fn quality_state(root: &Path, white: bool) -> State {
    let mut state = imported_state(
        root,
        if white {
            "(;SZ[9]PL[W];W[ec])"
        } else {
            "(;SZ[9];B[ec])"
        },
    );
    // Deliberately different root and best scores: colors compare alternatives,
    // while the existing numeric labels compare against the current position.
    let sign = if white { -1.0 } else { 1.0 };
    let losses = [0.0, 0.25, 1.0, 2.0, 8.0, 0.25, 8.0, 0.0, 0.5];
    let mut moves: Vec<_> = losses
        .into_iter()
        .enumerate()
        .map(|(x, loss)| katastro::SuggestedMove {
            point: Some(Point::new(x, 2)),
            visits: 32,
            winrate: Some(0.5),
            score_lead: Some(sign * (10.0 - loss)),
        })
        .collect();
    moves[5].visits = 1;
    moves[6].visits = 1;
    moves[7].score_lead = None;
    moves[7].winrate = None;
    moves[7].visits = 0;
    moves.push(katastro::SuggestedMove {
        point: None,
        visits: 32,
        winrate: Some(0.4),
        score_lead: Some(sign * -2.0),
    });
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 1024,
            winrate: 0.5,
            score_lead: sign * 2.0,
            suggestions: moves,
        },
    );
    state
}
fn candidate_fill(h: &Harness<'_, State>, label: &str) -> eframe::egui::Color32 {
    let rect = h
        .query_by_label(label)
        .expect("candidate marker is missing")
        .rect();
    h.output()
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            eframe::egui::Shape::Circle(c)
                if c.center.distance(rect.center()) < 0.1
                    && c.radius > rect.width() * 0.3
                    && c.fill.a() > 0 =>
            {
                Some(c.fill)
            }
            _ => None,
        })
        .expect("the candidate is not drawn as a filled circle")
}
#[test]
fn all_candidates_are_visible_on_board_and_a_later_played_candidate_has_one_delta() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(
            |ui, state: &mut State| state.render(ui),
            quality_state(temp.path(), false),
        );
    for (rank, column) in "ABCDEFGHJ".chars().enumerate() {
        assert!(
            h.query_by_label(&format!("AI suggestion {}: {column}7", rank + 1))
                .is_some(),
            "the engine's candidate beyond rank five was dropped"
        );
    }
    let played = h.get_by_label("Next recorded move: Black E7").rect();
    assert_eq!(
        painted_text_in(&h, played),
        vec!["+0.0"],
        "the played fifth candidate needs one label"
    );
    if let Some(path) = std::env::var_os("KATASTRO_CANDIDATE_PREVIEW") {
        h.run();
        h.render().unwrap().save(path).unwrap();
    }
    // Move the played candidate beyond the previous five-move cap.
    h.state_mut()
        .snapshot
        .values
        .get_mut(&0)
        .unwrap()
        .suggestions
        .swap(4, 8);
    h.run();
    assert!(h.query_by_label("AI suggestion 9: E7").is_some());
    assert_eq!(
        painted_text_in(&h, played),
        vec!["+0.0"],
        "candidate/played overlap drew duplicate values"
    );
    h.get_by_label("Play H7").click();
    h.run();
    let id = h.state().review.document().unwrap().selected;
    assert_eq!(
        h.state()
            .snapshot
            .board
            .as_ref()
            .unwrap()
            .stone(Point::new(7, 2)),
        Some(Color::Black)
    );
    drop(h);
    let reopened = quality_state(temp.path(), false);
    assert_eq!(reopened.review.document().unwrap().selected, id);
    assert_eq!(reopened.review.document().unwrap().nodes.len(), 3);
}
#[test]
fn candidate_gradient_uses_mover_relative_loss_not_rank_or_root_delta() {
    for white in [false, true] {
        let temp = tempfile::TempDir::new().unwrap();
        let mut h = Harness::builder()
            .with_size(eframe::egui::vec2(1200.0, 1100.0))
            .build_ui_state(
                |ui, state: &mut State| state.render(ui),
                quality_state(temp.path(), white),
            );
        let best = candidate_fill(&h, "AI suggestion 1: A7");
        assert!(best.b() > best.r() && best.b() > best.g());
        let good = candidate_fill(&h, "AI suggestion 2: B7");
        assert!(
            good.g() > good.r() && good.g() > good.b(),
            "a near-equal alternative should be green"
        );
        let yellow = candidate_fill(&h, "AI suggestion 3: C7");
        let orange = candidate_fill(&h, "AI suggestion 4: D7");
        let bad = candidate_fill(&h, "AI suggestion 5: E7");
        assert_ne!(yellow, good, "a full point loss is still painted green");
        assert_ne!(
            orange, yellow,
            "different losses have identical rank-only colors"
        );
        assert!(yellow.r() > yellow.b() && yellow.g() > yellow.b());
        assert!(orange.r() > orange.g() && orange.g() > orange.b());
        assert!(
            u16::from(bad.r()) > u16::from(bad.g()) * 2
                && u16::from(bad.r()) > u16::from(bad.b()) * 2,
            "an eight-point blunder is not red"
        );
        assert_eq!(
            painted_text_in(&h, h.get_by_label("AI suggestion 5: E7").rect()),
            vec!["+0.0"],
            "colors must improve without changing the requested current-position delta"
        );
        h.state_mut()
            .snapshot
            .values
            .get_mut(&0)
            .unwrap()
            .suggestions[2]
            .score_lead = Some(if white { -8.99 } else { 8.99 });
        h.run();
        let adjusted = candidate_fill(&h, "AI suggestion 3: C7");
        assert_ne!(
            adjusted, yellow,
            "the gradient does not respond to a small assessment refinement"
        );
        assert!(
            adjusted
                .to_array()
                .iter()
                .zip(yellow.to_array())
                .all(|(a, b)| a.abs_diff(b) < 5),
            "a tiny refinement caused a large color jump"
        );
    }
}
#[test]
fn low_visit_candidates_are_subdued_and_unknown_scores_are_neutral_until_refined() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(
            |ui, state: &mut State| state.render(ui),
            quality_state(temp.path(), false),
        );
    let reliable_good = candidate_fill(&h, "AI suggestion 2: B7");
    let weak_good = candidate_fill(&h, "AI suggestion 6: F7");
    let reliable_bad = candidate_fill(&h, "AI suggestion 5: E7");
    let weak_bad = candidate_fill(&h, "AI suggestion 7: G7");
    assert!(
        weak_good.a() < reliable_good.a(),
        "one visit has the same emphasis as a searched alternative"
    );
    assert!(weak_bad.a() < reliable_bad.a());
    assert!(
        weak_bad.r() > weak_bad.g() && weak_bad.r() > weak_bad.b(),
        "uncertainty must not turn bad moves green"
    );
    let unknown = candidate_fill(&h, "AI suggestion 8: H7");
    assert_eq!(
        unknown.r(),
        unknown.g(),
        "a policy preview is claiming evaluated strength"
    );
    assert_eq!(unknown.g(), unknown.b());
    h.state_mut()
        .snapshot
        .values
        .get_mut(&0)
        .unwrap()
        .suggestions[7]
        .score_lead = Some(-2.0);
    h.state_mut()
        .snapshot
        .values
        .get_mut(&0)
        .unwrap()
        .suggestions[7]
        .visits = 32;
    h.run();
    let refined = candidate_fill(&h, "AI suggestion 8: H7");
    assert!(u16::from(refined.r()) > u16::from(refined.g()) * 2);
    assert_eq!(
        painted_text_in(&h, h.get_by_label("AI suggestion 8: H7").rect()),
        vec!["-4.0"]
    );
}
#[test]
fn scarce_legal_choices_do_not_turn_bad_candidates_green_and_policy_best_is_neutral() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = imported_state(temp.path(), "(;SZ[2]AB[aa][ab]AW[ba]PL[W])");
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 256,
            winrate: 0.1,
            score_lead: 10.0,
            suggestions: vec![
                katastro::SuggestedMove {
                    point: Some(Point::new(1, 1)),
                    visits: 32,
                    score_lead: Some(10.0),
                    winrate: Some(0.1),
                },
                katastro::SuggestedMove {
                    point: None,
                    visits: 32,
                    score_lead: Some(20.0),
                    winrate: Some(0.01),
                },
            ],
        },
    );
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let pass = h.get_by_label("2  Pass");
    let bounds = pass.rect();
    let pass_text = h
        .output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            eframe::egui::Shape::Text(t)
                if bounds.contains(t.pos) && t.galley.text() == "2  Pass" =>
            {
                Some(t.galley.job.sections[0].format.color)
            }
            _ => None,
        })
        .unwrap();
    assert!(
        u16::from(pass_text.r()) > u16::from(pass_text.g()) * 2,
        "the poor alternative to the only board move was not colored by strength"
    );
    h.get_by_label("2  Pass").click();
    h.run();
    assert!(
        h.state()
            .review
            .document()
            .unwrap()
            .nodes
            .last()
            .unwrap()
            .played
            .unwrap()
            .point
            .is_none()
    );
    h.get_by_label("Previous move").click();
    h.run();
    let moves = &mut h
        .state_mut()
        .snapshot
        .values
        .get_mut(&0)
        .unwrap()
        .suggestions;
    moves.truncate(1);
    moves[0].visits = 0;
    moves[0].score_lead = None;
    moves[0].winrate = None;
    h.run();
    let unknown = candidate_fill(&h, "AI suggestion 1: B1");
    assert_eq!(unknown.r(), unknown.g());
    assert_eq!(unknown.g(), unknown.b());
    let bounds = h.get_by_label("AI suggestion 1: B1").rect();
    assert!(
        has_circle(&h, bounds, |c| c.fill == eframe::egui::Color32::TRANSPARENT
            && c.stroke.color == eframe::egui::Color32::from_rgb(65, 145, 255)
            && c.radius > bounds.width() * 0.3),
        "the leading policy preview lost its blue outline"
    );
}
#[test]
fn uncapped_candidate_sidebar_scrolls_without_moving_charts_and_creates_persistent_moves() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = quality_state(temp.path(), false);
    state
        .snapshot
        .values
        .get_mut(&0)
        .unwrap()
        .suggestions
        .extend((0..9).map(|x| katastro::SuggestedMove {
            point: Some(Point::new(x, 4)),
            visits: 32,
            winrate: Some(0.5),
            score_lead: Some(9.75),
        }));
    let mut h = Harness::builder()
        .with_max_steps(60)
        .with_size(eframe::egui::vec2(1200.0, 1100.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let score = h.get_by_label("Score lead chart").rect();
    let winrate = h.get_by_label("Winrate chart").rect();
    let viewport = h
        .query_by_label("AI candidate list")
        .expect("all-candidate list is absent")
        .rect();
    assert!(viewport.height() <= 148.0);
    h.event(eframe::egui::Event::PointerMoved(viewport.center()));
    h.event(eframe::egui::Event::MouseWheel {
        unit: eframe::egui::MouseWheelUnit::Point,
        delta: eframe::egui::vec2(0.0, -2000.0),
        phase: eframe::egui::TouchPhase::Move,
        modifiers: eframe::egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.get_by_label("Score lead chart").rect(), score);
    assert_eq!(h.get_by_label("Winrate chart").rect(), winrate);
    let last = h.get_by_label("19  J5");
    assert!(
        viewport.contains_rect(last.rect()),
        "later candidates were not reachable in the list"
    );
    last.click();
    h.run();
    assert_eq!(
        h.state()
            .snapshot
            .board
            .as_ref()
            .unwrap()
            .stone(Point::new(8, 4)),
        Some(Color::Black)
    );
    let selected = h.state().review.document().unwrap().selected;
    drop(h);
    assert_eq!(
        quality_state(temp.path(), false)
            .review
            .document()
            .unwrap()
            .selected,
        selected
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

fn painted_text_in(h: &Harness<'_, State>, bounds: eframe::egui::Rect) -> Vec<String> {
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            eframe::egui::Shape::Text(text) if bounds.contains(text.pos) => {
                Some(text.galley.text().to_owned())
            }
            _ => None,
        })
        .collect()
}
fn marker_radius(h: &Harness<'_, State>, bounds: eframe::egui::Rect, dotted: bool) -> f32 {
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            eframe::egui::Shape::Circle(circle)
                if !dotted && bounds.contains(circle.center) && circle.fill.a() == 255 =>
            {
                Some(circle.radius)
            }
            eframe::egui::Shape::Path(path)
                if dotted && !path.closed && path.points.iter().all(|p| bounds.contains(*p)) =>
            {
                path.points.first().map(|p| p.distance(bounds.center()))
            }
            _ => None,
        })
        .fold(0.0, f32::max)
}
#[test]
fn ai_and_recorded_move_markers_have_matching_circles_and_pending_score_labels() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    let mut policy = suggestions();
    for s in &mut policy {
        s.score_lead = None;
        s.winrate = None;
        s.visits = 0;
    }
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 1,
            winrate: 0.5,
            score_lead: 2.5,
            suggestions: policy,
        },
    );
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 860.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let suggested = h.get_by_label("AI suggestion 1: D6").rect();
    let played = h.get_by_label("Next recorded move: Black C7").rect();
    assert!(
        (marker_radius(&h, suggested, false) - marker_radius(&h, played, true)).abs() < 0.01,
        "candidate and recorded move circles have different sizes"
    );
    for bounds in [suggested, played] {
        assert!(
            painted_text_in(&h, bounds).contains(&"--".to_string()),
            "missing score must have a placeholder, not a rank or a fabricated zero"
        );
    }
}

fn evaluated_state(root: &Path, white: bool) -> State {
    let mut state = imported_state(
        root,
        if white {
            "(;SZ[9]PL[W];W[cc])"
        } else {
            "(;SZ[9];B[cc])"
        },
    );
    let mut moves = suggestions();
    moves[0].score_lead = Some(if white { 1.2 } else { 3.7 });
    moves[1].score_lead = Some(if white { 4.0 } else { 1.0 });
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 256,
            winrate: 0.6,
            score_lead: 2.5,
            suggestions: moves,
        },
    );
    state.snapshot.values.insert(
        1,
        katastro::Analysis {
            visits: 64,
            winrate: 0.4,
            score_lead: if white { 4.5 } else { 0.0 },
            suggestions: vec![],
        },
    );
    state
}
#[test]
fn move_circles_compare_signed_score_changes_for_black_and_white() {
    for white in [false, true] {
        let temp = tempfile::TempDir::new().unwrap();
        let h = Harness::builder()
            .with_size(eframe::egui::vec2(1200.0, 860.0))
            .build_ui_state(
                |ui, state: &mut State| state.render(ui),
                evaluated_state(temp.path(), white),
            );
        for (label, expected) in [
            ("AI suggestion 1: D6", if white { "+1.3" } else { "+1.2" }),
            ("AI suggestion 2: E5", "-1.5"),
            (
                if white {
                    "Next recorded move: White C7"
                } else {
                    "Next recorded move: Black C7"
                },
                if white { "-2.0" } else { "-2.5" },
            ),
        ] {
            let rect = h.get_by_label(label).rect();
            assert_eq!(
                painted_text_in(&h, rect),
                vec![expected],
                "{label} should show its point change from the mover's perspective"
            );
        }
    }
}
#[test]
fn a_played_ai_candidate_has_one_label_and_uses_the_same_search_estimate() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = imported_state(temp.path(), "(;SZ[9];B[dd])");
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 1024,
            winrate: 0.6,
            score_lead: 2.5,
            suggestions: suggestions(),
        },
    );
    // An older child search must not give a different label to the very same candidate.
    state.snapshot.values.insert(
        1,
        katastro::Analysis {
            visits: 64,
            winrate: 0.5,
            score_lead: -20.0,
            suggestions: vec![],
        },
    );
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 860.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let suggested = h.get_by_label("AI suggestion 1: D6").rect();
    let played = h.get_by_label("Next recorded move: Black D6").rect();
    assert_eq!(
        painted_text_in(&h, played),
        vec!["+0.0"],
        "the same move needs one consistent score label"
    );
    assert!(
        has_circle(&h, suggested, |c| c.fill.b() > c.fill.r()
            && c.fill.b() > c.fill.g()),
        "the best move must remain blue when it was also played"
    );
    h.get_by_label("Play D6").click();
    h.run();
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().selected, 1);
    assert_eq!(h.state().snapshot.document.as_ref().unwrap().nodes.len(), 2);
}
#[test]
fn recorded_delta_refines_and_survives_cache_reopen_without_an_engine() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = evaluated_state(temp.path(), false);
    let profile = katastro::EngineProfile {
        model_sha256: "model".into(),
        engine_sha256: "engine".into(),
        settings_digest: "black".into(),
    };
    for (node, value) in &state.snapshot.values {
        let key = state.review.analysis_key(*node, &profile).unwrap();
        state.review.store_analysis(&key, value).unwrap();
    }
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 860.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    let ring = h.get_by_label("Next recorded move: Black C7").rect();
    assert_eq!(painted_text_in(&h, ring), vec!["-2.5"]);
    h.state_mut()
        .snapshot
        .values
        .get_mut(&1)
        .unwrap()
        .score_lead = 1.5;
    h.state_mut().snapshot.values.get_mut(&1).unwrap().visits = 1024;
    h.run();
    assert_eq!(painted_text_in(&h, ring), vec!["-1.0"]);
    let key = h.state().review.analysis_key(1, &profile).unwrap();
    let value = h.state().snapshot.values[&1].clone();
    h.state_mut().review.store_analysis(&key, &value).unwrap();
    drop(h);
    let mut reopened = imported_state(temp.path(), "(;SZ[9];B[cc])");
    reopened.snapshot.values = reopened.review.cached_analysis(&profile).unwrap();
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1200.0, 860.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), reopened);
    let ring = h.get_by_label("Next recorded move: Black C7").rect();
    assert_eq!(painted_text_in(&h, ring), vec!["-1.0"]);
    assert_eq!(
        painted_text_in(&h, h.get_by_label("AI suggestion 1: D6").rect()),
        vec!["+1.2"]
    );
}

#[test]
fn delta_labels_fit_inside_equal_circles_on_a_19_by_19_board() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = imported_state(temp.path(), "(;SZ[19];B[cc])");
    let mut moves = suggestions();
    moves[0].score_lead = Some(100.5);
    moves[1].score_lead = Some(-107.5);
    state.snapshot.values.insert(
        0,
        katastro::Analysis {
            visits: 256,
            winrate: 0.5,
            score_lead: 2.5,
            suggestions: moves,
        },
    );
    state.snapshot.values.insert(
        1,
        katastro::Analysis {
            visits: 64,
            winrate: 0.5,
            score_lead: 0.0,
            suggestions: vec![],
        },
    );
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(900.0, 680.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    for (label, expected) in [
        ("AI suggestion 1: D16", "+98.0"),
        ("AI suggestion 2: E15", "-110.0"),
        ("Next recorded move: Black C17", "-2.5"),
    ] {
        let bounds = h.get_by_label(label).rect();
        assert_eq!(painted_text_in(&h, bounds), vec![expected]);
        for shape in &h.output().shapes {
            if let eframe::egui::Shape::Text(text) = &shape.shape
                && bounds.contains(text.pos)
            {
                let text_rect = text.galley.rect.translate(text.pos.to_vec2());
                assert!(
                    bounds.contains_rect(text_rect),
                    "score label spills outside its circle: {text_rect:?} {bounds:?}"
                );
            }
        }
    }
}
