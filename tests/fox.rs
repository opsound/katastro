use katastro::{
    Color, Document, Point, Review,
    engine::{Engine, EngineConfig, EngineEvent},
    scheduler::Scheduler,
};
use std::time::{Duration, Instant};

// Synthetic moves and names; reproduces Fox's duplicated/non-composed AP metadata.
const FOX: &str = "(;GM[1]FF[4]SZ[19]PB[测试黑]PW[Test White]KM[0]HA[1]RU[Chinese]AP[GNU Go:3.8]RN[2]RE[W+0]TM[300]TC[3]TT[30]AP[foxwq]RL[0];B[dd];W[pp];B[dp];W[pd])";

fn encoded_fox() -> String {
    FOX.replace("KM[0]HA[1]", "KM[375]HA[0]")
}

fn analyze_fox(config: EngineConfig, sgf: &str, komi: f64) {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("fox.sgf");
    std::fs::write(&source, sgf).unwrap();
    let reviews = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let mut review = Review::new(&reviews, &cache).unwrap();
    review.import(&source).expect("Fox SGF must import");
    let doc = review.document().unwrap().clone();
    assert_eq!(doc.nodes[0].property("PB"), Some("测试黑"));
    assert_eq!(doc.position(0).unwrap().initial_player, "B");
    assert_eq!(
        doc.board(4).unwrap().stone(Point::new(15, 3)),
        Some(Color::White)
    );
    let profile = Engine::profile(&config).unwrap();
    let mut scheduler = Scheduler::new(1, doc.clone(), profile.clone(), Default::default());
    let mut engine = Engine::spawn(&config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    for budget in [1, 8, 64] {
        while scheduler.values.len() < doc.nodes.len()
            || scheduler.values.values().any(|a| a.visits < budget)
        {
            let request = scheduler.next_request().unwrap().unwrap();
            assert_eq!(request.visits, budget);
            assert_eq!(
                request.query["komi"], komi,
                "engine must receive point komi"
            );
            assert_eq!(request.query["rules"], "chinese");
            engine.send(&request.query).unwrap();
            while scheduler.pending() > 0 {
                match engine
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .unwrap()
                {
                    EngineEvent::Response(reply) => {
                        for (target, value) in scheduler.accept(&reply).unwrap() {
                            if reply["isDuringSearch"] == false {
                                review.store_analysis(&target.key, &value).unwrap();
                            }
                        }
                    }
                    EngineEvent::Diagnostic(_) => {}
                    event => panic!("unexpected engine event: {event:?}"),
                }
            }
            assert_eq!(scheduler.coverage(), (5, 5));
        }
        assert!(scheduler.values.values().all(|v| v.visits >= budget));
        assert!(
            scheduler.values[&0].score_lead.abs() < 20.0,
            "empty even-game opening has an extreme point balance: {:?}",
            scheduler.values[&0]
        );
    }
    review.select(1).unwrap();
    let branch = review.play(Some(Point::new(4, 4))).unwrap();
    let export = temp.path().join("export.sgf");
    review.export(&export).unwrap();
    let exported = std::fs::read_to_string(export).unwrap();
    assert!(exported.contains(&format!("KM[{komi}]")));
    let exported = Document::parse(exported.as_bytes()).unwrap();
    assert_eq!(exported.komi, komi, "export must not convert komi twice");
    assert_eq!(exported.mainline.len(), 5);
    drop(review);
    let mut reopened = Review::new(&reviews, &cache).unwrap();
    reopened.import(&source).unwrap();
    assert_eq!(reopened.document().unwrap().selected, branch);
    assert_eq!(reopened.document().unwrap().mainline, doc.mainline);
    let cached = reopened.cached_analysis(&profile).unwrap();
    assert_eq!(cached, scheduler.values);
    let mut restored = Scheduler::new(2, doc, profile, cached);
    assert!(
        restored.next_request().unwrap().unwrap().visits > 1,
        "cached game must skip the cheap pass"
    );
    assert_eq!(std::fs::read_to_string(source).unwrap(), sgf);
}

#[test]
fn fox_import_analysis_export_and_cached_reopen() {
    let temp = tempfile::TempDir::new().unwrap();
    let model = temp.path().join("model");
    std::fs::write(&model, b"fake").unwrap();
    analyze_fox(
        EngineConfig {
            analysis: Default::default(),
            executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
            model,
        },
        FOX,
        0.0,
    );
}

#[test]
fn fox_encoded_komi_analysis_export_and_cached_reopen() {
    let temp = tempfile::TempDir::new().unwrap();
    let model = temp.path().join("model");
    std::fs::write(&model, b"fake").unwrap();
    analyze_fox(
        EngineConfig {
            analysis: Default::default(),
            executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
            model,
        },
        &encoded_fox(),
        7.5,
    );
}

#[test]
fn fox_komi_correction_preserves_legacy_variations_but_excludes_wrong_komi_cache() {
    use katastro::{Analysis, EngineProfile};
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("fox.sgf");
    let legacy_source = temp.path().join("legacy.sgf");
    let reviews = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let sgf = encoded_fox();
    std::fs::write(&source, &sgf).unwrap();
    // Create the old 375-point review through public commands, then restore its
    // Fox metadata to simulate a database written before import normalization.
    std::fs::write(&legacy_source, sgf.replace("AP[foxwq]", "AP[other:1]")).unwrap();
    let profile = EngineProfile {
        model_sha256: "model".into(),
        engine_sha256: "engine".into(),
        settings_digest: "black".into(),
    };
    let mut old = Review::new(&reviews, &cache).unwrap();
    old.import(&legacy_source).unwrap();
    let legacy_id = old.document().unwrap().source_identity().unwrap();
    let mainline = old.document().unwrap().mainline.clone();
    old.select(1).unwrap();
    old.play(Some(Point::new(4, 4))).unwrap();
    let branch = old.play(Some(Point::new(5, 5))).unwrap();
    let board = old.document().unwrap().board(branch).unwrap();
    let wrong_key = old.analysis_key(0, &profile).unwrap();
    old.store_analysis(
        &wrong_key,
        &Analysis {
            visits: 16384,
            score_lead: -200.0,
            winrate: 0.0,
            suggestions: vec![],
            ownership: vec![],
            ownership_visits: 0,
        },
    )
    .unwrap();
    let mut legacy = old.document().unwrap().clone();
    legacy.nodes[0]
        .props
        .insert("AP".into(), vec!["foxwq".into()]);
    drop(old);
    rusqlite::Connection::open(&reviews)
        .unwrap()
        .execute(
            "UPDATE documents SET document=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&legacy).unwrap(), legacy_id],
        )
        .unwrap();

    let mut corrected = Review::new(&reviews, &cache).unwrap();
    corrected.import(&source).unwrap();
    assert!(
        corrected.cached_analysis(&profile).unwrap().is_empty(),
        "375-point komi analysis must never populate the corrected chart"
    );
    let doc = corrected.document().unwrap();
    assert_eq!(doc.selected, branch, "saved variation selection was lost");
    assert_eq!(doc.mainline, mainline);
    assert_eq!(doc.board(branch).unwrap(), board);
    assert_eq!(doc.komi, 7.5);
    let key = corrected.analysis_key(0, &profile).unwrap();
    assert_ne!(key, wrong_key);
    let value = Analysis {
        visits: 64,
        score_lead: -0.5,
        winrate: 0.48,
        suggestions: vec![],
        ownership: vec![],
        ownership_visits: 0,
    };
    corrected.store_analysis(&key, &value).unwrap();
    corrected.select(2).unwrap();
    drop(corrected);
    let mut reopened = Review::new(&reviews, &cache).unwrap();
    reopened.import(&source).unwrap();
    assert_eq!(reopened.document().unwrap().selected, 2);
    assert_eq!(reopened.document().unwrap().board(branch).unwrap(), board);
    assert_eq!(reopened.cached_analysis(&profile).unwrap()[&0], value);
    let export = temp.path().join("export.sgf");
    reopened.export(&export).unwrap();
    assert!(std::fs::read_to_string(export).unwrap().contains("KM[7.5]"));
    assert_eq!(std::fs::read_to_string(source).unwrap(), sgf);
}

#[test]
fn only_recognized_fox_komi_encodings_are_normalized() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("source.sgf");
    let canonical = temp.path().join("canonical.sgf");
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    for (metadata, expected) in [
        ("RU[Chinese]AP[GNU Go:3.8]AP[foxwq]KM[375]", 7.5),
        ("RU[Chinese]AP[foxwq]AP[GNU Go:3.8]KM[325]", 6.5),
        ("RU[Japanese]AP[foxwq:1.0]KM[650]", 6.5),
        ("RU[Chinese]AP[foxwq]KM[7.5]", 7.5),
        ("RU[Japanese]AP[foxwq]KM[6.5]", 6.5),
        ("RU[Chinese]AP[foxwq]KM[0]HA[1]", 0.0),
        ("RU[Chinese]AP[foxwq]KM[0.5]HA[2]AB[dd][pp]", 0.5),
        ("RU[Chinese]AP[foxwq]KM[-7.5]", -7.5),
        ("RU[Chinese]AP[other:1]KM[375]", 375.0),
        ("RU[Chinese]C[AP[foxwq\\]]KM[375]", 375.0),
        ("RU[Chinese]AP[foxwq]KM[100]", 100.0),
    ] {
        let sgf = format!("(;SZ[19]{metadata};B[cc];W[qq])");
        std::fs::write(&source, &sgf).unwrap();
        review.import(&source).unwrap();
        let mut schedule = Scheduler::new(
            1,
            review.document().unwrap().clone(),
            katastro::EngineProfile {
                model_sha256: "m".into(),
                engine_sha256: "e".into(),
                settings_digest: "s".into(),
            },
            Default::default(),
        );
        assert_eq!(
            schedule.next_request().unwrap().unwrap().query["komi"],
            expected,
            "{metadata}"
        );
        review.export(&canonical).unwrap();
        assert!(
            std::fs::read_to_string(&canonical)
                .unwrap()
                .contains(&format!("KM[{expected}]"))
        );
        review.import(&canonical).unwrap();
        assert_eq!(review.document().unwrap().komi, expected, "{metadata}");
        assert_eq!(std::fs::read_to_string(&source).unwrap(), sgf);
    }
}

#[test]
#[ignore = "Requires KATASTRO_TEST_ENGINE and KATASTRO_TEST_MODEL; run with --ignored"]
fn live_fox_analysis_and_cache() {
    for (sgf, komi) in [(FOX.to_owned(), 0.0), (encoded_fox(), 7.5)] {
        analyze_fox(
            EngineConfig {
                analysis: Default::default(),
                executable: std::env::var_os("KATASTRO_TEST_ENGINE")
                    .expect("KATASTRO_TEST_ENGINE")
                    .into(),
                model: std::env::var_os("KATASTRO_TEST_MODEL")
                    .expect("KATASTRO_TEST_MODEL")
                    .into(),
            },
            &sgf,
            komi,
        );
    }
}

#[test]
#[ignore = "Requires KATASTRO_TEST_SGF, KATASTRO_TEST_ENGINE and KATASTRO_TEST_MODEL"]
fn live_local_fox_worker_import_and_analysis() {
    use katastro::worker::{Client, Command};
    let source =
        std::path::PathBuf::from(std::env::var_os("KATASTRO_TEST_SGF").expect("KATASTRO_TEST_SGF"));
    let bytes = std::fs::read(&source).unwrap();
    let doc = Document::parse(&bytes).expect("local Fox SGF must import");
    assert!(doc.board(0).unwrap().cells.iter().all(Option::is_none));
    let positions = doc.mainline.len();
    assert!(doc.board(*doc.mainline.last().unwrap()).is_ok());
    let temp = tempfile::TempDir::new().unwrap();
    let client = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        EngineConfig {
            analysis: Default::default(),
            executable: std::env::var_os("KATASTRO_TEST_ENGINE")
                .expect("KATASTRO_TEST_ENGINE")
                .into(),
            model: std::env::var_os("KATASTRO_TEST_MODEL")
                .expect("KATASTRO_TEST_MODEL")
                .into(),
        },
    );
    client.recv(Duration::from_secs(5)).unwrap();
    client
        .send(Command::OpenAndAnalyze(source.clone()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let state = client
            .recv(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(state.error.is_none(), "{:?}", state.error);
        if state.coverage == (positions, positions) {
            assert!(state.values.values().all(|v| v.visits > 0));
            let opening = &state.values[&0];
            assert!(
                opening.score_lead.abs() < 20.0,
                "opening point balance {} at {} visits (komi {})",
                opening.score_lead,
                opening.visits,
                doc.komi
            );
            if opening.visits >= 64 {
                println!(
                    "Local Fox opening: {:+.3} points at {} visits; komi {}",
                    opening.score_lead, opening.visits, doc.komi
                );
                break;
            }
        }
    }
    client.send(Command::Pause).unwrap();
    assert_eq!(std::fs::read(source).unwrap(), bytes);
    println!("Local Fox game: {positions} positions imported and analyzed");
}
