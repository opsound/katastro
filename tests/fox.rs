use katastro::{
    Color, Document, Point, Review,
    engine::{Engine, EngineConfig, EngineEvent},
    scheduler::Scheduler,
};
use std::time::{Duration, Instant};

// Synthetic moves and names; reproduces Fox's duplicated/non-composed AP metadata.
const FOX: &str = "(;GM[1]FF[4]SZ[19]PB[测试黑]PW[Test White]KM[0]HA[1]RU[Chinese]AP[GNU Go:3.8]RN[2]RE[W+0]TM[300]TC[3]TT[30]AP[foxwq]RL[0];B[dd];W[pp];B[dp];W[pd])";

fn analyze_fox(config: EngineConfig) {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("fox.sgf");
    std::fs::write(&source, FOX).unwrap();
    let reviews = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let mut review = Review::new(&reviews, &cache).unwrap();
    review.import(&source).expect("Fox SGF must import");
    let doc = review.document().unwrap().clone();
    assert_eq!(doc.nodes[0].property("PB"), Some("测试黑"));
    assert_eq!(doc.komi, 0.0);
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
        let request = scheduler.next_request().unwrap().unwrap();
        assert_eq!(request.visits, budget);
        assert_eq!(request.query["komi"], 0.0);
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
        assert!(scheduler.values.values().all(|v| v.visits >= budget));
    }
    review.select(1).unwrap();
    let branch = review.play(Some(Point::new(4, 4))).unwrap();
    let export = temp.path().join("export.sgf");
    review.export(&export).unwrap();
    assert_eq!(
        Document::parse(&std::fs::read(export).unwrap())
            .unwrap()
            .mainline
            .len(),
        5
    );
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
    assert_eq!(std::fs::read_to_string(source).unwrap(), FOX);
}

#[test]
fn fox_import_analysis_export_and_cached_reopen() {
    let temp = tempfile::TempDir::new().unwrap();
    let model = temp.path().join("model");
    std::fs::write(&model, b"fake").unwrap();
    analyze_fox(EngineConfig {
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model,
    });
}

#[test]
#[ignore = "Requires KATASTRO_TEST_ENGINE and KATASTRO_TEST_MODEL; run with --ignored"]
fn live_fox_analysis_and_cache() {
    analyze_fox(EngineConfig {
        executable: std::env::var_os("KATASTRO_TEST_ENGINE")
            .expect("KATASTRO_TEST_ENGINE")
            .into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL")
            .expect("KATASTRO_TEST_MODEL")
            .into(),
    });
}

#[test]
#[ignore = "Requires KATASTRO_TEST_SGF, KATASTRO_TEST_ENGINE and KATASTRO_TEST_MODEL"]
fn live_local_fox_worker_import_and_analysis() {
    use katastro::worker::{Client, Command};
    let source =
        std::path::PathBuf::from(std::env::var_os("KATASTRO_TEST_SGF").expect("KATASTRO_TEST_SGF"));
    let bytes = std::fs::read(&source).unwrap();
    let doc = Document::parse(&bytes).expect("local Fox SGF must import");
    let positions = doc.mainline.len();
    assert!(doc.board(*doc.mainline.last().unwrap()).is_ok());
    let temp = tempfile::TempDir::new().unwrap();
    let client = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        EngineConfig {
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
            break;
        }
    }
    client.send(Command::Pause).unwrap();
    assert_eq!(std::fs::read(source).unwrap(), bytes);
    println!("Local Fox game: {positions} positions imported and analyzed");
}
