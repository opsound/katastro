#![cfg(feature = "desktop")]

use egui_kittest::{Harness, kittest::Queryable};
use katastro::{desktop::DesktopApp, worker::Snapshot};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

fn wait(h: &mut Harness<'_, DesktopApp>, ready: impl Fn(&Snapshot) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        h.step();
        if ready(&h.state().state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "waiting for review update: {:?}",
            h.state().state.error
        );
        std::thread::yield_now();
    }
}

fn at_move(h: &mut Harness<'_, DesktopApp>, number: usize) {
    wait(h, |s| {
        s.board.as_ref().is_some_and(|b| b.move_number == number)
    });
}

// Listening to clicks instead of accepted plays would also sound on suicide,
// while comparing selected nodes would sound when returning to saved positions.
#[test]
fn accepted_stones_click_once_and_muting_survives_reopening() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("position.sgf");
    std::fs::write(&source, b"(;SZ[9]AW[ab][ba];B[cc])").unwrap();
    let clicks = Rc::new(Cell::new(0));
    let build = || {
        let clicks = clicks.clone();
        Harness::builder()
            .with_size(eframe::egui::vec2(1200., 820.))
            .build_eframe(|cc| {
                DesktopApp::new(
                    &cc.egui_ctx,
                    temp.path().join("reviews"),
                    temp.path().join("cache"),
                    Default::default(),
                    Some(source.clone()),
                )
                .with_stone_sound(move || clicks.set(clicks.get() + 1))
            })
    };
    let mut h = build();
    at_move(&mut h, 0);
    h.get_by_label("Play A9").click();
    wait(&mut h, |s| s.error.is_some());
    assert_eq!(clicks.get(), 0, "suicide made a sound");
    h.get_by_label("Next move").click();
    at_move(&mut h, 1);
    h.get_by_label("Previous move").click();
    at_move(&mut h, 0);
    assert_eq!(clicks.get(), 0, "navigation made a sound");

    h.get_by_label("Play C7").click(); // Following the imported move by placing a stone.
    at_move(&mut h, 1);
    h.run_steps(3);
    assert_eq!(
        clicks.get(),
        1,
        "accepted placement should click exactly once"
    );
    h.key_press(eframe::egui::Key::P);
    at_move(&mut h, 2);
    assert_eq!(clicks.get(), 1, "pass made a sound");
    h.get_by_label("Mute stones").click();
    h.run_steps(2);
    h.get_by_label("Play D6").click();
    at_move(&mut h, 3);
    assert_eq!(clicks.get(), 1, "muted variation made a sound");
    drop(h);

    let mut h = build();
    at_move(&mut h, 3);
    h.get_by_label("Play E5").click();
    at_move(&mut h, 4);
    assert_eq!(clicks.get(), 1, "mute did not survive restart");
    h.get_by_label("Training").click();
    h.run_steps(2);
    h.get_by_label("Mute stones").click();
    h.run_steps(2);
    h.get_by_label("Back to review").click();
    h.run_steps(2);
    h.get_by_label("Play F4").click();
    at_move(&mut h, 5);
    assert_eq!(clicks.get(), 2, "unmuting did not restore the sound");
}
