#![cfg(all(target_os = "macos", feature = "desktop"))]
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{Document, desktop::DesktopApp, engine::EngineConfig};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

#[test]
fn chosen_human_ranks_drive_legal_responses_and_a_reviewable_game() {
    for rank in ["20k", "5d"] {
        let temp = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let model = temp.path().join("model");
        std::fs::write(
            &model,
            format!("controlled:{}", listener.local_addr().unwrap()),
        )
        .unwrap();
        let cfg = EngineConfig {
            model,
            executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
            ..Default::default()
        };
        let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
        let output = clicks.clone();
        let mut ui = Harness::builder()
            .with_size(eframe::egui::vec2(1200., 820.))
            .build_eframe(|cc| {
                DesktopApp::with_archives(
                    &cc.egui_ctx,
                    temp.path().join("reviews"),
                    temp.path().join("cache"),
                    cfg,
                    None,
                    vec![],
                )
                .with_stone_sound(move || output.set(output.get() + 1))
            });
        ui.get_by_label("Play AI").click();
        ui.run();
        ui.get_by_role(eframe::egui::accesskit::Role::ComboBox)
            .click();
        ui.run();
        let item = ui.get_by_label(rank);
        item.scroll_to_me();
        ui.run();
        ui.get_by_label(rank).click();
        ui.run();
        ui.get_by_label("Start game").click();
        ui.run();
        let until = |h: &mut Harness<'_, DesktopApp>, label: &str| {
            let end = Instant::now() + Duration::from_secs(10);
            while h.query_by_label(label).is_none() {
                h.step();
                assert!(Instant::now() < end, "missing {label}");
            }
        };
        until(&mut ui, "Your turn");
        assert_eq!(clicks.get(), 0);
        // This rank starts muted and unmutes before its AI reply.
        if rank == "5d" {
            ui.get_by_label("Mute stones").click();
            ui.run();
        }
        ui.get_by_label("Play D4").click();
        ui.run();
        listener.set_nonblocking(true).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < end => {
                    std::thread::yield_now()
                }
                Err(e) => panic!("waiting for engine connection: {e}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut io = BufReader::new(stream);
        for turn in [1, 3] {
            until(&mut ui, &format!("KataGo ({rank}) is thinking"));
            if turn == 1 {
                assert_eq!(clicks.get(), usize::from(rank == "20k"), "human placement");
                if rank == "5d" {
                    ui.get_by_label("Mute stones").click();
                    ui.run();
                }
            }
            let mut line = String::new();
            io.read_line(&mut line).unwrap();
            let query: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(
                query["overrideSettings"]["humanSLProfile"],
                format!("rank_{rank}")
            );
            assert_eq!(query["maxVisits"], 1);
            assert_eq!(query["includePolicy"], true);
            assert_eq!(query["overrideSettings"]["ignorePreRootHistory"], false);
            let moves = if turn == 1 {
                json!([["B", "D4"]])
            } else {
                json!([["B", "D4"], ["W", "K10"], ["B", "pass"]])
            };
            assert_eq!(query["moves"], moves);

            let mut policy = vec![0.0; 362];
            // D4 is occupied. The only legal weighted move is K10, then pass.
            policy[15 * 19 + 3] = 0.9;
            policy[if turn == 1 { 180 } else { 361 }] = 0.1;
            let reply = json!({"id":query["id"],"turnNumber":turn,"isDuringSearch":false,"humanPolicy":policy});
            let mut delayed = reply.clone();
            delayed["id"] = json!("unrelated");
            delayed["humanPolicy"] = json!(vec![0.0; 362]);
            delayed["humanPolicy"][0] = json!(1.0);
            writeln!(io.get_mut(), "{delayed}").unwrap();
            delayed["id"] = query["id"].clone();
            delayed["turnNumber"] = json!(0);
            writeln!(io.get_mut(), "{delayed}").unwrap();
            writeln!(io.get_mut(), "{reply}").unwrap();
            if turn == 1 {
                until(&mut ui, "Your turn");
                ui.get_by_label("Pass").click();
                ui.run();
            }
        }
        until(&mut ui, "Both players passed");
        assert_eq!(
            clicks.get(),
            1 + usize::from(rank == "20k"),
            "AI placement and two silent passes"
        );
        ui.get_by_label("Review game").click();
        let end = Instant::now() + Duration::from_secs(10);
        while ui.state().state.source.is_none() {
            ui.step();
            assert!(Instant::now() < end);
        }
        let saved =
            Document::parse(&std::fs::read(ui.state().state.source.as_ref().unwrap()).unwrap())
                .unwrap();
        assert_eq!(saved.mainline.len(), 5);
        assert_eq!(saved.nodes[0].property("RE"), Some("W+7.5"));
        assert_eq!(saved.nodes[0].property("WR"), Some(rank));
        assert!(
            saved.nodes[3..]
                .iter()
                .all(|n| n.played.unwrap().point.is_none())
        );
    }
}

#[test]
fn area_score_counts_stones_and_single_color_territory_and_leaves_dame_neutral() {
    let doc = Document::parse(b"(;SZ[5]AB[ba][bb][bc][bd][be][ca]AW[da][db][dc][dd][de])").unwrap();
    assert_eq!(doc.board(0).unwrap().area_lead(7.5), -6.5);
}

#[test]
#[ignore = "requires the bundled macOS engine with a working backend"]
fn a_real_ranked_opponent_can_open_as_black_and_review_a_resignation() {
    let temp = tempfile::tempdir().unwrap();
    let cfg = EngineConfig::discover(temp.path());
    let mut ui = Harness::builder()
        .with_size(eframe::egui::vec2(1200., 820.))
        .build_eframe(|cc| {
            DesktopApp::with_archives(
                &cc.egui_ctx,
                temp.path().join("reviews"),
                temp.path().join("cache"),
                cfg,
                None,
                vec![],
            )
        });
    ui.get_by_label("Play AI").click();
    ui.run();
    ui.get_by_label("White").click();
    ui.run();
    ui.get_by_label("9×9").click();
    ui.run();
    ui.get_by_label("Start game").click();
    ui.run();
    let deadline = Instant::now() + Duration::from_secs(120);
    while ui.query_by_label("Your turn").is_none() {
        ui.step();
        assert!(
            Instant::now() < deadline,
            "waiting for the first real HumanSL move"
        );
    }
    ui.get_by_label("Resign").click();
    while ui.query_by_label("Review game").is_none() {
        ui.step();
        assert!(Instant::now() < deadline, "waiting for resignation");
    }
    ui.get_by_label("Review game").click();
    while ui.state().state.source.is_none() {
        ui.step();
        assert!(Instant::now() < deadline, "waiting for saved game");
    }
    let saved = ui.state().state.document.as_ref().unwrap();
    assert_eq!(saved.size, 9);
    assert_eq!(saved.nodes[0].property("RE"), Some("B+R"));
    assert_eq!(saved.nodes[0].property("BR"), Some("5k"));
    assert_eq!(saved.mainline.len(), 2);
    assert_eq!(saved.nodes[1].played.unwrap().color, katastro::Color::Black);
}
