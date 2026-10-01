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
fn has_strength_ring(h: &Harness<'_, State>, gtp: &str, hue: &str) -> bool {
    let rect = h.get_by_label(&format!("Play {gtp}")).rect();
    h.output().shapes.iter().any(|shape| match &shape.shape {
        Shape::Circle(circle) => {
            circle.center.distance(rect.center()) < 0.5
                && circle.radius > rect.width() * 0.25
                && circle.radius < rect.width() * 0.45
                && circle.fill == Color32::TRANSPARENT
                && is_color(circle.stroke.color, hue)
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
fn stone_fills(h: &Harness<'_, State>, gtp: &str) -> Vec<(f32, Color32)> {
    let rect = h.get_by_label(&format!("Play {gtp}")).rect();
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Circle(circle)
                if circle.center.distance(rect.center()) < 0.5
                    && circle.radius > rect.width() * 0.4
                    && circle.fill != Color32::TRANSPARENT =>
            {
                Some((circle.radius, circle.fill))
            }
            _ => None,
        })
        .collect()
}
#[test]
fn strength_overlay_preserves_black_and_white_stone_fills_for_every_status() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state.snapshot.values.insert(
        0,
        analysis(
            64,
            &[
                ("B8", 0.9),
                ("C8", 0.7),
                ("D6", -0.98),
                ("E5", 0.0),
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
    let pairs = [("B8", "G3"), ("D6", "A3"), ("E5", "J9")];
    let before: Vec<_> = pairs
        .iter()
        .flat_map(|(black, white)| [*black, *white])
        .map(|point| (point, stone_fills(&h, point)))
        .collect();
    for (black, white) in pairs {
        let black = stone_fills(&h, black);
        let white = stone_fills(&h, white);
        assert_eq!(black.len(), 1);
        assert_eq!(white.len(), 1);
        assert!(black[0].1.r() < 50 && white[0].1.r() > 235);
    }
    h.get_by_label("Group strength").click();
    h.run();
    for (point, fills) in before {
        assert_eq!(
            stone_fills(&h, point),
            fills,
            "group strength must leave the original stone fill unchanged at {point}"
        );
    }
    assert!(
        !colored_segments(&h).is_empty(),
        "strength outlines remain visible"
    );
    h.get_by_label("Play F6").click();
    h.run();
    let branch = h.state().snapshot.document.as_ref().unwrap().selected;
    drop(h);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&temp.path().join("game.sgf")).unwrap();
    assert_eq!(
        reopened
            .document()
            .unwrap()
            .board(branch)
            .unwrap()
            .stone(Point::from_gtp("F6", 9).unwrap().unwrap()),
        Some(Color::Black),
        "unfilled strength rings must keep variation input and autosave working"
    );
}
#[test]
fn group_boundaries_are_thin_and_keep_their_contrast_halos() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state
        .snapshot
        .values
        .insert(0, analysis(64, &[("B8", 0.9), ("C8", 0.7), ("D6", -0.98)]));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 860.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    h.get_by_label("Group strength").click();
    h.run();
    let colored = colored_segments(&h);
    for hue in ["green", "orange", "red"] {
        assert!(
            colored
                .iter()
                .any(|(_, stroke)| is_color(stroke.color, hue))
        );
    }
    for (points, stroke) in colored {
        assert!(
            stroke.width <= 1.6,
            "colored group boundaries should be light enough to keep the stones visually dominant"
        );
        let halo = h
            .output()
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                Shape::LineSegment {
                    points: underneath,
                    stroke: dark,
                } if underneath == &points
                    && dark.width > stroke.width
                    && dark.color.r() < 60
                    && dark.color.g() < 60
                    && dark.color.b() < 60 =>
                {
                    Some(dark)
                }
                _ => None,
            })
            .expect("thin outlines must retain a contrast halo");
        assert!(halo.width <= 3.2, "group halos should also be thinner");
    }
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
    assert!(
        !has_strength_ring(&h, "B8", "green"),
        "the overlay must start off"
    );
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
            has_strength_ring(&h, point, hue),
            "missing {hue} strength ring at {point}"
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
        has_strength_ring(&h, "B8", "orange") && has_strength_ring(&h, "C8", "orange"),
        "connected stones must use their chain's mean reading"
    );
    h.get_by_label("Group strength").click();
    h.run();
    assert!(
        !has_strength_ring(&h, "B8", "orange"),
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
        !has_strength_ring(&h, "B8", "orange"),
        "parent ownership must disappear on an unanalyzed child"
    );
    h.get_by_label("Group estimates pending");
    h.state_mut().snapshot.values.insert(
        branch,
        analysis(64, &[("B8", -0.9), ("C8", -0.9), ("F6", 0.95)]),
    );
    h.run();
    assert!(
        has_strength_ring(&h, "B8", "red") && has_strength_ring(&h, "F6", "green"),
        "new variation estimates must replace the parent overlay"
    );
    h.get_by_label("Tree node 0").click();
    h.run();
    assert!(
        has_strength_ring(&h, "B8", "orange"),
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
        assert!(!has_strength_ring(&h, "B8", hue));
    }
    let before = h.get_by_label("Play B8").rect();
    h.state_mut()
        .snapshot
        .values
        .insert(0, analysis(4096, &[("B8", 0.99), ("C8", 0.99)]));
    h.run();
    assert!(has_strength_ring(&h, "B8", "green"));
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

#[test]
fn group_rings_have_uniform_color_on_black_and_white_and_outlines_have_dark_halos() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    state.snapshot.values.insert(
        0,
        analysis(
            64,
            &[
                ("B8", 0.9),
                ("C8", 0.7),
                ("D6", -0.98),
                ("E5", 0.0),
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
    h.get_by_label("Group strength").click();
    h.run();
    for (black, white, hue) in [
        ("B8", "G3", "green"),
        ("D6", "A3", "red"),
        ("E5", "J9", "orange"),
    ] {
        let mut colors = Vec::new();
        for point in [black, white] {
            let rect = h.get_by_label(&format!("Play {point}")).rect();
            let ring = h
                .output()
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    Shape::Circle(c)
                        if c.center.distance(rect.center()) < 0.5
                            && c.radius > rect.width() * 0.25
                            && c.radius < rect.width() * 0.45
                            && c.stroke.width >= 2.0
                            && is_color(c.stroke.color, hue) =>
                    {
                        Some(c)
                    }
                    _ => None,
                })
                .expect("colored status ring must be visible inside the stone");
            assert_eq!(
                ring.stroke.color.a(),
                255,
                "rings must retain their color over either stone background"
            );
            assert!(
                ring.stroke.width <= 2.0,
                "inscribed strength rings must be half their former maximum thickness"
            );
            colors.push(ring.stroke.color);
        }
        assert_eq!(
            colors[0], colors[1],
            "equal readings must have equally vivid status rings on Black and White stones"
        );
    }
    let colored = colored_segments(&h);
    for (points, stroke) in colored
        .iter()
        .filter(|(_, s)| is_color(s.color, "red") || is_color(s.color, "orange"))
    {
        assert!(
            h.output().shapes.iter().any(|s| match &s.shape {
                Shape::LineSegment {
                    points: halo,
                    stroke: dark,
                } =>
                    halo == points
                        && dark.width > stroke.width + 1.0
                        && dark.color.r() < 60
                        && dark.color.g() < 60
                        && dark.color.b() < 60
                        && dark.color.a() > 200,
                _ => false,
            }),
            "weak chain outlines must be separated from the warm board by a dark halo"
        );
    }
    h.get_by_label("Play F6").click();
    h.run();
    assert!(h.state().snapshot.document.as_ref().unwrap().nodes.len() > 2);
}

#[test]
fn quick_group_depth_is_visible_without_claiming_the_deep_chart_depth() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = state(temp.path());
    let mut value = analysis(16384, &[("B8", 0.9), ("C8", 0.9)]);
    value.ownership_visits = 1;
    state.snapshot.values.insert(0, value);
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 680.0))
        .build_ui_state(|ui, state: &mut State| state.render(ui), state);
    h.get_by_label("Group strength").click();
    h.run();
    assert!(has_strength_ring(&h, "B8", "green"));
    assert!(
        h.query_by_label("· quick estimate · 1 visit").is_some(),
        "a quick group map must show its actual effort rather than the chart's 16384 visits"
    );
    let before = h.get_by_label("Play B8").rect();
    h.state_mut()
        .snapshot
        .values
        .get_mut(&0)
        .unwrap()
        .ownership_visits = 64;
    h.run();
    assert!(h.query_by_label("· 64 visits").is_some());
    assert_eq!(h.get_by_label("Play B8").rect(), before);
}
