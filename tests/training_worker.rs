use katastro::{
    digest,
    engine::EngineConfig,
    training::{
        Phase, Settings, Snapshot,
        client::{Client, Command, Options},
    },
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};
#[path = "support/training.rs"]
mod support;
fn until(client: &Client, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let s = client
            .recv(end.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(s.error.is_none(), "{:?}", s.error);
        if done(&s) {
            return s;
        }
    }
}
fn setup(root: &std::path::Path, listener: &TcpListener) -> (Options, std::path::PathBuf) {
    let source = root.join("game.sgf");
    std::fs::write(
        &source,
        b"(;SZ[9]PB[me]PW[opponent];B[aa];W[ba];B[ca];W[da])",
    )
    .unwrap();
    let model = root.join("model");
    std::fs::write(
        &model,
        format!("controlled:{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let config = EngineConfig {
        analysis: Default::default(),
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model,
    };
    let mut options = Options::new(root.join("reviews"), root.join("cache"), config);
    options.human_model = root.join("human");
    std::fs::write(&options.human_model, b"human-fixture").unwrap();
    options.model_sha256 = digest(b"human-fixture");
    (options, source)
}
fn query(control: &mut BufReader<TcpStream>) -> Value {
    let mut line = String::new();
    control.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

// The UI must be notified of startup errors as well as progress, even before
// an engine exists. This protects the shared background-publishing refactor.
#[test]
fn training_storage_failure_publishes_and_wakes_for_every_update() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (options, source) = setup(temp.path(), &listener);
    std::fs::create_dir(&options.reviews).unwrap();
    let (notify, notified) = std::sync::mpsc::channel();
    let client = Client::spawn(
        options,
        vec![source],
        "me".into(),
        Settings::default(),
        Some(std::sync::Arc::new(move || {
            let _ = notify.send(());
        })),
    );
    loop {
        notified
            .recv_timeout(Duration::from_secs(5))
            .expect("training updates must wake the UI");
        let state = client
            .snapshots
            .try_recv()
            .expect("publish the snapshot before waking");
        if state.error.is_some() {
            assert_eq!(state.phase, Phase::Idle);
            assert!(state.status.starts_with("Training stopped"));
            break;
        }
        assert_eq!(state.phase, Phase::Searching);
    }
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err());
}
// Benefit: library preparation and a full attempt run through a real subprocess,
// while user commands stay responsive; cancellation must not apply a delayed AI move.
#[test]
fn background_training_resumes_before_grading_and_cancels_a_thinking_opponent() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (options, source) = setup(temp.path(), &listener);
    let client = Client::spawn(
        options,
        vec![source],
        "me".into(),
        Settings {
            moves: 2,
            strict: true,
            ..Settings::default()
        },
        None,
    );
    until(&client, |s| s.phase == Phase::Searching);
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut engine = BufReader::new(stream);
    // Five screening positions, with replies in reverse order.
    let mut queries = Vec::new();
    for _ in 0..5 {
        queries.push(query(&mut engine));
    }
    for q in queries.iter().rev() {
        finish(&mut engine, q);
    }
    for _ in 0..2 {
        let q = query(&mut engine);
        finish(&mut engine, &q);
    }
    until(&client, |s| s.phase == Phase::YourTurn);
    client
        .send(Command::Play(Some(katastro::Point::new(3, 4))))
        .unwrap();
    let human = loop {
        let q = query(&mut engine);
        if q["includePolicy"] == true {
            break q;
        }
        finish(&mut engine, &q);
    };
    assert_eq!(human["overrideSettings"]["humanSLProfile"], "rank_1d");
    until(&client, |s| s.phase == Phase::Opponent);
    // Hold the normal evaluation, but deliver the much shorter HumanSL reply.
    let grading = query(&mut engine);
    assert_eq!(grading["maxVisits"], 512);
    let mut policy = vec![-1.0; 82];
    policy[81] = 1.0;
    writeln!(
        engine.get_mut(),
        "{}",
        json!({"id":human["id"],"turnNumber":1,"isDuringSearch":false,"humanPolicy":policy})
    )
    .unwrap();
    let ready = until(&client, |s| s.phase == Phase::YourTurn);
    assert_eq!(ready.board.unwrap().move_number, 2);
    assert!(ready.report.is_none());
    client
        .send(Command::Play(Some(katastro::Point::new(4, 4))))
        .unwrap();
    loop {
        let q = query(&mut engine);
        if q["includePolicy"] == true {
            assert_eq!(q["moves"].as_array().unwrap().len(), 3);
            break;
        }
    }
    until(&client, |s| s.phase == Phase::Opponent);
    client.send(Command::Cancel).unwrap();
    let stopped = until(&client, |s| s.phase == Phase::Idle);
    assert!(stopped.document.is_none());
}
fn finish(engine: &mut BufReader<TcpStream>, q: &Value) {
    writeln!(engine.get_mut(), "{}", support::reply(q, false, false)).unwrap();
}

// Benefit: Retry and Next release unfinished graph searches before sending new
// work to the same engine. Canceled replies cannot alter the new attempt.
#[test]
fn leaving_a_report_cancels_chart_work_before_the_next_attempt() {
    for next in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (options, source) = setup(temp.path(), &listener);
        let client = Client::spawn(
            options,
            vec![source],
            "me".into(),
            Settings {
                moves: 1,
                ..Default::default()
            },
            None,
        );
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (events, received) = std::sync::mpsc::channel();
        let serving = std::thread::spawn(move || {
            let mut engine = BufReader::new(stream);
            let mut held = vec![];
            let mut hold = true;
            loop {
                let mut line = String::new();
                match engine.read_line(&mut line) {
                    Ok(0) => break,
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
                    Ok(_) => {}
                    Err(e) => panic!("controlled engine connection: {e}"),
                }
                let q: Value = serde_json::from_str(&line).unwrap();
                if q["action"] == "terminate_all" {
                    events.send("canceled").unwrap();
                    hold = false;
                    for stale in held.drain(..) {
                        finish(&mut engine, &stale);
                    }
                    continue;
                }
                let turn = q["moves"].as_array().unwrap().len();
                if hold && q["includePolicy"] != true && q["maxVisits"] == 512 && turn == 2 {
                    held.push(q);
                    events.send("endpoint").unwrap();
                    continue;
                }
                let response = if q["includePolicy"] == true {
                    let mut policy = vec![-1.0; 82];
                    policy[81] = 1.0;
                    json!({"id":q["id"],"turnNumber":turn,"isDuringSearch":false,"humanPolicy":policy})
                } else {
                    let leading = turn.is_multiple_of(2);
                    let mut r = support::reply(&q, false, false);
                    r["rootInfo"]["winrate"] = json!(if leading { 0.8 } else { 0.4 });
                    r["rootInfo"]["scoreLead"] = json!(if leading { 8.0 } else { -2.0 });
                    r
                };
                if writeln!(engine.get_mut(), "{response}").is_err() {
                    break;
                }
            }
        });
        until(&client, |s| s.phase == Phase::YourTurn);
        client
            .send(Command::Play(Some(katastro::Point::new(3, 4))))
            .unwrap();
        until(&client, |s| s.phase == Phase::Failed);
        for _ in 0..2 {
            assert_eq!(
                received.recv_timeout(Duration::from_secs(5)).unwrap(),
                "endpoint"
            );
        }
        client
            .send(if next { Command::Next } else { Command::Retry })
            .unwrap();
        assert_eq!(
            received
                .recv_timeout(Duration::from_secs(5))
                .expect("unfinished chart searches were not canceled"),
            "canceled"
        );
        let ready = until(&client, |s| s.phase == Phase::YourTurn);
        assert!(ready.report.is_none());
        assert_eq!(ready.board.unwrap().move_number, if next { 2 } else { 0 });
        drop(client);
        serving.join().unwrap();
    }
}

#[test]
fn model_download_is_verified_installed_atomically_and_reused_without_network() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (mut options, _) = setup(temp.path(), &listener);
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    options.model_url = format!("http://{}/model", server.server_addr());
    std::fs::remove_file(&options.human_model).unwrap();
    options.model_sha256 = digest(b"downloaded-human-fixture");
    let serving = std::thread::spawn(move || {
        let r = server.recv().unwrap();
        r.respond(tiny_http::Response::from_data(b"downloaded-human-fixture"))
            .unwrap();
    });
    let model_path = options.human_model.clone();
    let url = options.model_url.clone();
    let client = Client::spawn(options, vec![], "me".into(), Settings::default(), None);
    until(&client, |s| s.phase == Phase::Idle);
    serving.join().unwrap();
    assert_eq!(
        std::fs::read(&model_path).unwrap(),
        b"downloaded-human-fixture"
    );
    let (mut options, _) = setup(temp.path(), &listener);
    std::fs::write(&options.human_model, b"downloaded-human-fixture").unwrap();
    options.model_sha256 = digest(b"downloaded-human-fixture");
    options.model_url = url;
    let client = Client::spawn(options, vec![], "me".into(), Settings::default(), None);
    until(&client, |s| s.phase == Phase::Idle);
    assert!(
        std::fs::read_dir(temp.path()).unwrap().all(|e| e
            .unwrap()
            .path()
            .extension()
            .is_none_or(|s| s != "part"))
    );
}
#[test]
fn a_corrupted_model_download_cannot_start_the_engine_or_replace_the_installed_file() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (mut options, source) = setup(temp.path(), &listener);
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    options.model_url = format!("http://{}/model", server.server_addr());
    options.model_sha256 = digest(b"expected");
    let model_path = options.human_model.clone();
    let serving = std::thread::spawn(move || {
        server
            .recv()
            .unwrap()
            .respond(tiny_http::Response::from_data(b"corrupted"))
            .unwrap();
    });
    let client = Client::spawn(
        options,
        vec![source],
        "me".into(),
        Settings::default(),
        None,
    );
    loop {
        let s = client.recv(Duration::from_secs(5)).unwrap();
        if let Some(error) = s.error {
            assert!(error.contains("checksum"));
            break;
        }
    }
    serving.join().unwrap();
    assert_eq!(std::fs::read(model_path).unwrap(), b"human-fixture");
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err());
}

// Run only with local library permission, KataGo >=1.15 and the HumanSL model.
// The account name is read from sync.sqlite; no credential or Keychain access.
#[test]
#[ignore = "requires KATASTRO_TRAINING_LIBRARY, KATASTRO_TEST_ENGINE, KATASTRO_TEST_MODEL and KATASTRO_TEST_HUMAN_MODEL; uses private local games"]
fn real_library_exercise_plays_grades_and_reveals_legal_lines() {
    let library =
        std::path::PathBuf::from(std::env::var_os("KATASTRO_TRAINING_LIBRARY").expect("library"));
    let db = rusqlite::Connection::open_with_flags(
        library.join("sync.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let account: String = db
        .query_row("SELECT username FROM config LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let games: Vec<std::path::PathBuf> = db
        .prepare("SELECT filename FROM games WHERE username=?1 ORDER BY date DESC LIMIT 5")
        .unwrap()
        .query_map([&account], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| library.join(r.unwrap()))
        .collect();
    let hashes: Vec<_> = games
        .iter()
        .map(|p| digest(&std::fs::read(p).unwrap()))
        .collect();
    let temp = tempfile::tempdir().unwrap();
    // Optional identical screening-only cache for before/after latency measurements.
    // No grading results may be retained in this seed.
    if let Some(seed) = std::env::var_os("KATASTRO_TRAINING_SCREENING_CACHE") {
        std::fs::copy(seed, temp.path().join("cache")).unwrap();
        let cache = rusqlite::Connection::open(temp.path().join("cache")).unwrap();
        let deeper: i64 = cache
            .query_row("SELECT count(*) FROM analysis WHERE visits>=512", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            deeper, 0,
            "latency measurements must actually perform grading"
        );
    }
    let mut options = Options::new(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        EngineConfig {
            analysis: Default::default(),
            executable: std::env::var_os("KATASTRO_TEST_ENGINE").unwrap().into(),
            model: std::env::var_os("KATASTRO_TEST_MODEL").unwrap().into(),
        },
    );
    options.human_model = std::env::var_os("KATASTRO_TEST_HUMAN_MODEL")
        .unwrap()
        .into();
    let client = Client::spawn(options, games.clone(), account, Settings::default(), None);
    let deadline = Instant::now() + Duration::from_secs(600);
    let wait = |wanted: fn(&Snapshot) -> bool| {
        loop {
            let s = client
                .recv(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            assert!(s.error.is_none(), "{:?}", s.error);
            assert_ne!(
                s.phase,
                Phase::Idle,
                "No exercise was found in these five local games; choose a different corpus"
            );
            if wanted(&s) {
                break s;
            }
        }
    };
    let mut state = wait(|s| s.phase == Phase::YourTurn);
    let exercise_start = state.board.as_ref().unwrap().move_number;
    let mut timings = Vec::new();
    for _ in 0..5 {
        let doc = state.document.as_ref().unwrap();
        let before = state.board.as_ref().unwrap().move_number;
        let point = (0..doc.size * doc.size)
            .map(|i| Some(katastro::Point::new(i % doc.size, i / doc.size)))
            .find(|point| doc.clone().append_move(*point).is_ok())
            .unwrap_or(None);
        let started = Instant::now();
        let mut replied = None;
        client.send(Command::Play(point)).unwrap();
        loop {
            state = client
                .recv(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            assert!(state.error.is_none(), "{:?}", state.error);
            if state
                .board
                .as_ref()
                .is_some_and(|b| b.move_number >= before + 2)
            {
                replied.get_or_insert(started.elapsed());
            }
            if matches!(state.phase, Phase::YourTurn | Phase::Passed | Phase::Failed) {
                break;
            }
        }
        let elapsed = started.elapsed();
        timings.push(json!({"ai_reply_ms": replied.map(|d| d.as_millis()),
            "ready_ms": elapsed.as_millis(),
            "post_reply_wait_ms": replied.map(|d| (elapsed-d).as_millis()),
            "final_grade": state.report.is_some()}));
        if state.report.is_some() {
            break;
        }
    }
    eprintln!("Training turn timings: {}", json!(timings));
    assert!(state.report.is_some());
    if !state.report.as_ref().unwrap().comparison.complete() {
        state = wait(|s| s.report.as_ref().is_some_and(|r| r.comparison.complete()));
    }
    let comparison = &state.report.as_ref().unwrap().comparison;
    assert_eq!(
        comparison.attempt.len(),
        state.board.as_ref().unwrap().move_number - exercise_start + 1
    );
    assert_eq!(comparison.recorded.len(), comparison.attempt.len());
    client.send(Command::Reveal).unwrap();
    state = wait(|s| s.phase == Phase::Revealed);
    assert!(!state.lines.is_empty());
    let (path, index) = state.reveal.unwrap();
    let doc = katastro::Document::parse(&std::fs::read(path).unwrap()).unwrap();
    assert!(doc.nodes[doc.mainline[index]].children.len() > 1);
    for (path, hash) in games.iter().zip(hashes) {
        assert_eq!(
            digest(&std::fs::read(path).unwrap()),
            hash,
            "source game changed"
        );
    }
}

#[test]
fn training_worker_limits_discovery_to_the_configured_parallel_positions() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (mut options, source) = setup(temp.path(), &listener);
    options.engine.analysis.parallel_positions = 2;
    let client = Client::spawn(
        options,
        vec![source],
        "me".into(),
        Settings {
            moves: 1,
            ..Default::default()
        },
        None,
    );
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut engine = BufReader::new(stream);
    for _ in 0..2 {
        assert_eq!(query(&mut engine)["maxVisits"], 64);
    }
    client.send(Command::Cancel).unwrap();
    until(&client, |s| s.phase == Phase::Idle);
    let mut extra = String::new();
    match engine.read_line(&mut extra) {
        Ok(0) => {}
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => panic!("only two discovery requests may precede cancellation: {other:?} {extra}"),
    }
}
