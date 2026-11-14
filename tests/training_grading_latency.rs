//! Opt-in before/after measurement. A private seed freezes one real exercise;
//! both versions use identical positions, cache inputs and replayed HumanSL replies.
use katastro::{
    Document, Point, digest,
    engine::{Engine, EngineConfig, EngineEvent},
    training::{Phase, Settings, Trainer},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn trainer(root: &Path, config: &EngineConfig) -> Trainer {
    Trainer::new(
        &root.join("reviews"),
        &root.join("cache"),
        &root.join("Training"),
        Engine::profile(config).unwrap(),
    )
    .unwrap()
}
fn accept(t: &mut Trainer, e: &Engine, deadline: Instant) {
    accept_attempt(t, e, deadline, None, 0)
}
fn accept_attempt(
    t: &mut Trainer,
    e: &Engine,
    deadline: Instant,
    plan: Option<&[Option<Point>]>,
    start_ply: usize,
) {
    match e
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap()
    {
        EngineEvent::Response(mut r) => {
            if r.get("humanPolicy").is_some()
                && let Some(plan) = plan
            {
                let state = t.snapshot();
                let size = state.document.as_ref().unwrap().size;
                let move_number = state.board.unwrap().move_number;
                let point = plan[(move_number - start_ply - 1) / 2];
                let mut policy = vec![-1.0; size * size + 1];
                policy[point.map_or(size * size, |p| p.y * size + p.x)] = 1.0;
                r["humanPolicy"] = json!(policy);
            }
            t.accept(&r, 0.5).unwrap();
        }
        EngineEvent::Diagnostic(_) => {}
        other => panic!("engine stopped: {other:?}"),
    }
    assert!(Instant::now() < deadline);
}
fn seed(root: &Path, config: &EngineConfig) -> Value {
    let manifest = root.join("exercise.json");
    if manifest.exists() {
        return serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    }
    std::fs::create_dir_all(root).unwrap();
    let library = PathBuf::from(std::env::var_os("KATASTRO_TRAINING_LIBRARY").unwrap());
    let db = rusqlite::Connection::open_with_flags(
        library.join("sync.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let account: String = db
        .query_row("SELECT username FROM config LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let games: Vec<PathBuf> = db
        .prepare("SELECT filename FROM games WHERE username=?1 ORDER BY date DESC LIMIT 5")
        .unwrap()
        .query_map([&account], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| library.join(r.unwrap()))
        .collect();
    let temp = tempfile::tempdir().unwrap();
    if let Some(cache) = std::env::var_os("KATASTRO_TRAINING_SCREENING_CACHE") {
        std::fs::copy(cache, temp.path().join("cache")).unwrap();
    }
    let mut t = trainer(temp.path(), config);
    t.begin(games.clone(), account.clone(), Settings::default())
        .unwrap();
    let mut engine = Engine::spawn(config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(600);
    while t.snapshot().phase == Phase::Searching {
        while let Some(q) = t.next_request().unwrap() {
            engine.send(&q).unwrap();
        }
        accept(&mut t, &engine, deadline);
    }
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
    let d = t.snapshot().document.unwrap();
    let position = d.position(d.selected).unwrap();
    let source = games
        .iter()
        .find(|p| {
            let doc = Document::parse(&std::fs::read(p).unwrap()).unwrap();
            doc.mainline
                .iter()
                .any(|id| doc.position(*id).is_ok_and(|p| p == position))
        })
        .unwrap();
    let result = json!({"source":source,"account":account,"source_sha256":digest(&std::fs::read(source).unwrap()),"start":d.board(d.selected).unwrap().move_number});
    drop(t);
    drop(engine);
    // Restrict preparation to this already discovered candidate. This changes
    // only catalog scope, never evaluations or visits, and is shared by both versions.
    let db = rusqlite::Connection::open(temp.path().join("cache")).unwrap();
    db.execute(
        "UPDATE training_games SET result=json_set(result,'$.complete',json('true'))",
        [],
    )
    .unwrap();
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    drop(db);
    std::fs::copy(temp.path().join("cache"), root.join("seed.sqlite")).unwrap();
    std::fs::write(&manifest, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    result
}

#[test]
#[ignore = "requires private KATASTRO_GRADING_BENCH_ROOT, KATASTRO_TRAINING_LIBRARY, KataGo and both models; reports elapsed time without timing thresholds"]
fn real_preparation_and_final_grading_latency() {
    let root = PathBuf::from(std::env::var_os("KATASTRO_GRADING_BENCH_ROOT").unwrap());
    let config = EngineConfig {
        executable: std::env::var_os("KATASTRO_TEST_ENGINE").unwrap().into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL").unwrap().into(),
    };
    let human = PathBuf::from(std::env::var_os("KATASTRO_TEST_HUMAN_MODEL").unwrap());
    let fixture = seed(&root, &config);
    let source = PathBuf::from(fixture["source"].as_str().unwrap());
    assert_eq!(
        digest(&std::fs::read(&source).unwrap()),
        fixture["source_sha256"]
    );
    let mut results = vec![];
    for prepared in [false, true] {
        let plan_path = root.join("opponent-line.json");
        let plan: Option<Vec<Option<Point>>> = std::fs::read(&plan_path)
            .ok()
            .map(|b| serde_json::from_slice(&b).unwrap());
        let temp = tempfile::tempdir().unwrap();
        std::fs::copy(root.join("seed.sqlite"), temp.path().join("cache")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(600);
        let started = Instant::now();
        let mut preparation_requests = 0;
        if prepared {
            let mut engine = Engine::spawn(&config).unwrap();
            let mut t = trainer(temp.path(), &config);
            t.prepare(
                vec![source.clone()],
                fixture["account"].as_str().unwrap().into(),
                false,
            )
            .unwrap();
            while t.snapshot().phase != Phase::Idle {
                while let Some(q) = t.next_request().unwrap() {
                    assert_ne!(q["includePolicy"], true);
                    preparation_requests += 1;
                    engine.send(&q).unwrap();
                }
                if t.snapshot().phase != Phase::Idle {
                    accept(&mut t, &engine, deadline);
                }
            }
        }
        let preparation_ms = started.elapsed().as_millis();
        // The application uses a separate training engine. Reuse only persisted
        // results across preparation, not the preparer's in-memory NN cache.
        let mut engine = Engine::spawn_with_human(&config, &human).unwrap();
        let mut t = trainer(temp.path(), &config);
        t.begin(
            vec![source.clone()],
            fixture["account"].as_str().unwrap().into(),
            Settings::default(),
        )
        .unwrap();
        assert_eq!(t.snapshot().phase, Phase::YourTurn);
        let start_ply = t.snapshot().board.unwrap().move_number;
        let start_nodes = t.snapshot().document.unwrap().nodes.len();
        assert_eq!(start_ply, fixture["start"]);
        let began = Instant::now();
        let mut last_move = None;
        let mut last_reply = None;
        let mut graded = None;
        let mut evaluation_requests = 0;
        let mut human_requests = 0;
        loop {
            let state = t.snapshot();
            if state.phase == Phase::YourTurn {
                let d = state.document.as_ref().unwrap();
                let point = (0..d.size * d.size)
                    .map(|i| Some(Point::new(i % d.size, i / d.size)))
                    .find(|p| d.clone().append_move(*p).is_ok())
                    .unwrap_or(None);
                if state.played == 4 {
                    last_move = Some(began.elapsed());
                }
                t.play(point).unwrap();
            }
            if state.board.as_ref().unwrap().move_number == start_ply + 10 {
                last_reply.get_or_insert(began.elapsed());
            }
            if let Some(report) = &state.report {
                graded.get_or_insert(began.elapsed());
                if report.comparison.attempt.len() == 11 && report.comparison.recorded.len() == 11 {
                    let finished = began.elapsed();
                    let d = state.document.unwrap();
                    let played: Vec<_> = d
                        .path(d.selected)
                        .unwrap()
                        .into_iter()
                        .skip(start_nodes + 1)
                        .step_by(2)
                        .map(|id| d.nodes[id].played.unwrap().point)
                        .collect();
                    if let Some(plan) = &plan {
                        assert_eq!(&played, plan);
                    } else {
                        assert_eq!(played.len(), 5);
                        std::fs::write(&plan_path, serde_json::to_vec_pretty(&played).unwrap())
                            .unwrap();
                    }
                    results.push(json!({"prepared":prepared,"preparation_ms":preparation_ms,"preparation_requests":preparation_requests,
                        "attempt_ms":finished.as_millis(),"last_move_to_grade_ms":(graded.unwrap()-last_move.unwrap()).as_millis(),
                        "last_reply_to_grade_ms":(graded.unwrap()-last_reply.unwrap()).as_millis(),
                        "last_reply_to_full_charts_ms":(finished-last_reply.unwrap()).as_millis(),
                        "evaluation_requests":evaluation_requests,"human_requests":human_requests,
                        "position_sha256":digest(&serde_json::to_vec(&d.position(d.selected).unwrap()).unwrap()),"passed":report.passed}));
                    break;
                }
            }
            while let Some(q) = t.next_request().unwrap() {
                if q["includePolicy"] == true {
                    human_requests += 1;
                } else {
                    evaluation_requests += 1;
                }
                engine.send(&q).unwrap();
            }
            accept_attempt(&mut t, &engine, deadline, plan.as_deref(), start_ply);
        }
        assert_eq!(human_requests, 5);
        assert_eq!(
            digest(&std::fs::read(&source).unwrap()),
            fixture["source_sha256"]
        );
        eprintln!("Grading latency: {}", results.last().unwrap());
    }
    if let Some(output) = std::env::var_os("KATASTRO_GRADING_RESULTS") {
        std::fs::write(output, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }
}
