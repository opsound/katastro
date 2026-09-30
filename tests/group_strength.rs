#![cfg(feature = "desktop")]
use eframe::egui::{self, Color32, Pos2, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{
    Analysis, Color, Point, Review,
    engine::EngineConfig,
    ui,
    worker::{Command, Snapshot},
};

struct State {
    review: Review,
    snapshot: Snapshot,
}
impl State {
    fn sync(&mut self) {
        self.snapshot.document = self.review.document().cloned();
        self.snapshot.board = self.review.document().map(|d| d.board(d.selected).unwrap());
    }
    fn render(&mut self, ui: &mut egui::Ui) {
        for action in ui::render(ui, &self.snapshot) {
            if let ui::Action::Review(command) = action {
                match command {
                    Command::Play(point) => {
                        self.review.play(point).unwrap();
                    }
                    Command::Select(node) => {
                        self.review.select(node).unwrap();
                    }
                    _ => {}
                }
            }
        }
        self.sync();
    }
}
fn state(root: &std::path::Path) -> State {
    std::fs::write(
        root.join("game.sgf"),
        "(;SZ[9]AB[bb][cb][dd][ee]AW[gg][gh][ag][ia];B[ff])",
    )
    .unwrap();
    let mut review = Review::new(&root.join("reviews"), &root.join("cache")).unwrap();
    review.import(&root.join("game.sgf")).unwrap();
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
fn analysis(visits: u64, readings: &[(&str, f64)]) -> Analysis {
    let mut ownership = vec![0.0; 81];
    for (gtp, value) in readings {
        let point = Point::from_gtp(gtp, 9).unwrap().unwrap();
        ownership[point.y * 9 + point.x] = *value;
    }
    serde_json::from_value(serde_json::json!({"visits":visits,"score_lead":2.5,"winrate":0.6,"suggestions":[],"ownership":ownership})).unwrap()
}
fn is_color(color: Color32, hue: &str) -> bool {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    a > 0
        && match hue {
            "green" => g > r && g > b,
            "red" => r > g.saturating_mul(2) && r > b.saturating_mul(2),
            "orange" => r > g && g > b.saturating_mul(2),
            _ => false,
        }
}
fn has_tint(h: &Harness<'_, State>, gtp: &str, hue: &str) -> bool {
    let rect = h.get_by_label(&format!("Play {gtp}")).rect();
    h.output().shapes.iter().any(|shape| match &shape.shape {
        Shape::Circle(circle) => {
            circle.center.distance(rect.center()) < 0.5
                && circle.radius > rect.width() * 0.4
                && circle.fill.a() < 255
                && is_color(circle.fill, hue)
        }
        _ => false,
    })
}
fn colored_segments(h: &Harness<'_, State>) -> Vec<([Pos2; 2], egui::Stroke)> {
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::LineSegment { points, stroke }
                if ["green", "red", "orange"]
                    .iter()
                    .any(|hue| is_color(stroke.color, hue)) =>
            {
                Some((*points, *stroke))
            }
            _ => None,
        })
        .collect()
}
#[test]
fn toggle_colors_connected_chains_for_both_players_and_keeps_board_input_live() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state.snapshot.values.insert(
        0,
        analysis(
            256,
            &[
                ("B8", 0.9),
                ("C8", 0.7),
                ("D6", -0.98),
                ("E5", 0.98),
                ("G3", -0.9),
                ("G2", -0.7),
                ("A3", 0.98),
                ("J9", 0.0),
            ],
        ),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 860.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    assert!(!has_tint(&h, "B8", "green"), "the overlay must start off");
    h.get_by_label("Group strength").click();
    h.run();
    for (point, hue) in [
        ("B8", "green"),
        ("C8", "green"),
        ("D6", "red"),
        ("E5", "green"),
        ("G3", "green"),
        ("G2", "green"),
        ("A3", "red"),
        ("J9", "orange"),
    ] {
        assert!(
            has_tint(&h, point, hue),
            "missing {hue} strength tint at {point}"
        );
    }
    let b = h.get_by_label("Play B8").rect().center();
    let c = h.get_by_label("Play C8").rect().center();
    let gap = c.x - b.x;
    let segments = colored_segments(&h);
    assert!(
        segments
            .iter()
            .any(|(points, stroke)| is_color(stroke.color, "green")
                && points[0].distance(points[1]) > gap * 0.1),
        "chain boundaries must be drawn"
    );
    assert!(
        !segments.iter().any(
            |(points, _)| (points[0].x - (b.x + c.x) * 0.5).abs() < gap * 0.1
                && (points[1].x - points[0].x).abs() < 0.1
                && (points[0].y - b.y).abs() < gap * 0.55
        ),
        "a connected chain must have no interior border"
    );
    assert!(
        segments
            .iter()
            .any(|(points, stroke)| is_color(stroke.color, "green")
                && points[0].distance(b) < gap
                && points[0].distance(points[1]) < gap * 0.3),
        "a tentative chain must have a broken outline"
    );
    let certain = h.get_by_label("Play E5").rect().center();
    assert!(
        segments
            .iter()
            .any(|(points, stroke)| is_color(stroke.color, "green")
                && points[0].distance(certain) < gap
                && points[0].distance(points[1]) > gap * 0.8),
        "a decisive chain must have a solid outline"
    );
    if let Some(path) = std::env::var_os("KATASTRO_GROUP_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    // Changing one connected stone changes the reading of the whole strict chain.
    h.state_mut()
        .snapshot
        .values
        .insert(0, analysis(1024, &[("B8", 0.9), ("C8", -0.9)]));
    h.run();
    assert!(
        has_tint(&h, "B8", "orange") && has_tint(&h, "C8", "orange"),
        "connected stones must use their chain's mean reading"
    );
    h.get_by_label("Group strength").click();
    h.run();
    assert!(
        !has_tint(&h, "B8", "orange"),
        "turning the overlay off must restore stones"
    );
    h.get_by_label("Group strength").click();
    h.run();
    h.get_by_label("Play F6").click();
    h.run();
    let branch = h.state().snapshot.document.as_ref().unwrap().selected;
    assert_eq!(
        h.state()
            .snapshot
            .board
            .as_ref()
            .unwrap()
            .stone(Point::from_gtp("F6", 9).unwrap().unwrap()),
        Some(Color::Black)
    );
    assert!(
        !has_tint(&h, "B8", "orange"),
        "parent ownership must disappear on an unanalyzed child"
    );
    h.get_by_label("Group estimates pending");
    h.state_mut().snapshot.values.insert(
        branch,
        analysis(64, &[("B8", -0.9), ("C8", -0.9), ("F6", 0.95)]),
    );
    h.run();
    assert!(
        has_tint(&h, "B8", "red") && has_tint(&h, "F6", "green"),
        "new variation estimates must replace the parent overlay"
    );
    h.get_by_label("Tree node 0").click();
    h.run();
    assert!(
        has_tint(&h, "B8", "orange"),
        "the toggle must remain on while navigating"
    );
    drop(h);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&temp.path().join("game.sgf")).unwrap();
    assert!(
        reopened.document().unwrap().nodes.len() > 2,
        "overlay clicks must still autosave variations"
    );
}
#[test]
fn missing_ownership_is_pending_instead_of_inventing_weak_groups() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state.snapshot.values.insert(
        0,
        serde_json::from_value(serde_json::json!({"visits":1024,"score_lead":2.5,"winrate":0.6}))
            .unwrap(),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 680.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    h.get_by_label("Group strength").click();
    h.run();
    h.get_by_label("Group estimates pending");
    for hue in ["orange", "green", "red"] {
        assert!(!has_tint(&h, "B8", hue));
    }
    let before = h.get_by_label("Play B8").rect();
    h.state_mut()
        .snapshot
        .values
        .insert(0, analysis(4096, &[("B8", 0.99), ("C8", 0.99)]));
    h.run();
    assert!(has_tint(&h, "B8", "green"));
    assert_eq!(
        before,
        h.get_by_label("Play B8").rect(),
        "ownership arriving must not move the board"
    );
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
