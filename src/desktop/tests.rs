use super::*;
use crate::{Document, Point, training};
use egui_kittest::{Harness, kittest::Queryable};
use std::time::{Duration, Instant};

fn wait(h: &mut Harness<'_, DesktopApp>, predicate: impl Fn(&Snapshot) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.step();
        assert!(
            h.state().state.error.is_none(),
            "{:?}",
            h.state().state.error
        );
        if predicate(&h.state().state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "review worker did not respond: {} selected={:?} move={:?}",
            h.state().state.status,
            h.state().state.document.as_ref().map(|d| d.selected),
            h.state().state.board.as_ref().map(|b| b.move_number)
        );
        std::thread::yield_now();
    }
}
fn labels(h: &Harness<'_, DesktopApp>, coordinate: &str) -> Vec<String> {
    let bounds = h.get_by_label(&format!("Play {coordinate}")).rect();
    let center = bounds.shrink(bounds.width() * 0.3);
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t)
                if center.contains(egui::Rect::from_min_size(t.pos, t.galley.size()).center()) =>
            {
                Some(t.galley.text().to_owned())
            }
            _ => None,
        })
        .collect()
}
// Benefit: exploration shows the local sequence, including when navigating or
// adding a branch. Regressions could omit labels, use game-wide numbers, or retain
// labels from a different line. Exercise the real desktop buttons and worker.
#[test]
fn explored_lines_number_stones_from_the_exercise_and_follow_navigation() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("game.sgf");
    let sgf = b"(;SZ[9];B[aa];W[ii];B[bb])";
    std::fs::write(&source, sgf).unwrap();
    let source = source.canonicalize().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200., 820.))
        .build_eframe(|cc| {
            let mut app = DesktopApp::new(
                &cc.egui_ctx,
                temp.path().join("reviews"),
                temp.path().join("cache"),
                EngineConfig {
                    analysis: Default::default(),
                    executable: Default::default(),
                    model: Default::default(),
                },
                Some(source.clone()),
            );
            app.training_view.open = true;
            app.training_state = training::Snapshot {
                phase: training::Phase::Revealed,
                status: "Explore the suggested lines".into(),
                document: Some(Document::parse(sgf).unwrap()),
                reveal: Some((source.clone(), 2)),
                lines: vec![vec![
                    Some(Point::new(3, 3)),
                    Some(Point::new(4, 3)),
                    Some(Point::new(5, 3)),
                ]],
                ..Default::default()
            };
            app
        });
    wait(&mut h, |s| s.document.is_some());
    h.run();
    h.get_by_label("Explore line 1").click();
    wait(&mut h, |s| s.board.as_ref().unwrap().move_number == 5);
    for (coordinate, number) in [("D6", "1"), ("E6", "2"), ("F6", "3")] {
        assert_eq!(
            labels(&h, coordinate),
            vec![number],
            "missing relative move at {coordinate}"
        );
        let bounds = h.get_by_label(&format!("Play {coordinate}")).rect();
        let color = h
            .output()
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if bounds.contains(t.pos) && t.galley.text() == number => {
                    Some(t.galley.job.sections[0].format.color)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            color,
            if number == "2" {
                egui::Color32::from_rgb(30, 32, 35)
            } else {
                egui::Color32::WHITE
            }
        );
    }
    for label in [
        "Variation move 1: Black D6",
        "Variation move 2: White E6",
        "Variation move 3: Black F6",
    ] {
        h.get_by_label(label);
    }
    assert!(labels(&h, "A9").is_empty(), "pre-exercise stone numbered");
    assert!(labels(&h, "J1").is_empty(), "pre-exercise stone numbered");
    let last_bounds = h.get_by_label("Play F6").rect();
    assert!(
        !h.output().shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Circle(c) if last_bounds.contains(c.center)
        && c.radius < last_bounds.width() * 0.25 && c.stroke.width > 0.)),
        "last-move ring overlaps the number"
    );
    if let Some(path) = std::env::var_os("KATASTRO_NUMBERED_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    h.get_by_label("Previous move").click();
    wait(&mut h, |s| s.board.as_ref().unwrap().move_number == 4);
    assert_eq!(labels(&h, "D6"), vec!["1"]);
    assert_eq!(labels(&h, "E6"), vec!["2"]);
    h.get_by_label("Play G6").click();
    wait(&mut h, |s| s.board.as_ref().unwrap().move_number == 5);
    assert_eq!(labels(&h, "G6"), vec!["3"]);
    assert!(labels(&h, "F6").is_empty());
    h.get_by_label("Exercise position").click();
    wait(&mut h, |s| s.document.as_ref().unwrap().selected == 2);
    assert!(labels(&h, "D6").is_empty());
    h.get_by_label("Explore line 1").click();
    wait(&mut h, |s| s.board.as_ref().unwrap().move_number == 5);
    assert_eq!(labels(&h, "F6"), vec!["3"]);
    assert_eq!(
        h.state().state.document.as_ref().unwrap().mainline,
        vec![0, 1, 2, 3]
    );
    h.get_by_label("Back to review").click();
    wait(&mut h, |s| s.board.as_ref().unwrap().move_number == 5);
    // Wait for the worker's command barrier rather than relying on a frame delay.
    h.state().client.send(Command::Select(2)).unwrap();
    wait(&mut h, |s| s.document.as_ref().unwrap().selected == 2);
    h.state().client.send(Command::Select(6)).unwrap();
    wait(&mut h, |s| s.document.as_ref().unwrap().selected == 6);
    assert!(
        labels(&h, "F6").is_empty(),
        "numbering leaked into ordinary review"
    );
    assert_eq!(std::fs::read(&source).unwrap(), sgf);
}
