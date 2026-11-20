use katastro::{
    Analysis, Point, Review,
    engine::{Engine, EngineConfig},
    worker::{Client, Command, Snapshot},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    time::{Duration, Instant},
};

fn game(root: &Path) -> std::path::PathBuf {
    let source = root.join("game.sgf");
    let moves: String = (0..20)
        .map(|i| {
            format!(
                ";{}[{}{}]",
                if i % 2 == 0 { "B" } else { "W" },
                (b'a' + i % 9) as char,
                (b'a' + i / 9) as char
            )
        })
        .collect();
    std::fs::write(&source, format!("(;SZ[9]{moves})")).unwrap();
    source
}
fn config(root: &Path, listener: &TcpListener) -> EngineConfig {
    let model = root.join("model");
    std::fs::write(
        &model,
        format!("controlled:{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    EngineConfig {
        analysis: Default::default(),
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model,
    }
}
fn control(listener: &TcpListener) -> BufReader<TcpStream> {
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.set_nodelay(true).unwrap();
    BufReader::new(stream)
}
fn query(control: &mut BufReader<TcpStream>) -> Value {
    let mut line = String::new();
    control
        .read_line(&mut line)
        .expect("the engine needs more work while unfinished positions remain pending");
    serde_json::from_str(&line).unwrap()
}
fn send(control: &mut BufReader<TcpStream>, reply: Value) {
    writeln!(control.get_mut(), "{reply}").unwrap();
}
fn finish(control: &mut BufReader<TcpStream>, q: &Value, turn: usize) {
    send(
        control,
        json!({"id":q["id"],"turnNumber":turn,"isDuringSearch":false,
        "rootInfo":{"visits":q["maxVisits"],"winrate":0.6,"scoreLead":turn as f64},"ownership":vec![0.1;81]}),
    );
}
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

// Benefit: later positions get durable evaluations while a slow early search is
// unfinished. Waiting for a whole batch to drain would stall this workflow.
#[test]
fn worker_refills_while_a_slow_position_is_pending_and_preserves_cached_results() {
    let temp = tempfile::tempdir().unwrap();
    let source = game(temp.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = config(temp.path(), &listener);
    let profile = Engine::profile(&config).unwrap();
    let db = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let client = Client::spawn(db.clone(), cache.clone(), config);
    client
        .send(Command::OpenAndAnalyze(source.clone()))
        .unwrap();
    let mut engine = control(&listener);
    let coverage = query(&mut engine);
    for turn in 0..21 {
        finish(&mut engine, &coverage, turn);
    }
    let mut held = None;
    for _ in 0..150 {
        let q = query(&mut engine);
        assert!(q.get("action").is_none());
        for turn in q["analyzeTurns"].as_array().unwrap() {
            let turn = turn.as_u64().unwrap() as usize;
            if turn == 0 {
                assert!(
                    held.replace(q["id"].clone()).is_none(),
                    "must not duplicate pending work"
                );
            } else {
                finish(&mut engine, &q, turn);
            }
        }
        if q["analyzeTurns"].as_array().unwrap().contains(&json!(8))
            && q["maxVisits"].as_u64().unwrap() >= 64
        {
            break;
        }
    }
    let state = until(&client, |s| {
        s.values.get(&8).is_some_and(|a| a.visits >= 64)
    });
    assert_eq!(
        state.values[&0].visits, 1,
        "slow position remains genuinely unfinished"
    );
    client.send(Command::Pause).unwrap();
    until(&client, |s| !s.running);
    let mut reopened = Review::new(&db, &cache).unwrap();
    reopened.import(&source).unwrap();
    let saved = reopened.cached_analysis(&profile).unwrap();
    assert!(saved[&8].visits >= 64);
    assert!(!saved.contains_key(&0));
    assert_eq!(reopened.document().unwrap().mainline.len(), 21);
}

// Benefit: independent branches refine and persist alongside foreground work;
// changing selection replaces stale foreground work without stopping its peers.
#[test]
fn branch_refinement_continues_during_selection_and_stale_replies_are_ignored() {
    let temp = tempfile::tempdir().unwrap();
    let source = game(temp.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = config(temp.path(), &listener);
    let profile = Engine::profile(&config).unwrap();
    let db = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let mut review = Review::new(&db, &cache).unwrap();
    review.import(&source).unwrap();
    for id in 0..21 {
        review
            .store_analysis(
                &review.analysis_key(id, &profile).unwrap(),
                &Analysis {
                    visits: 64,
                    winrate: 0.5,
                    score_lead: 0.,
                    suggestions: vec![],
                    ownership: vec![0.; 81],
                    ownership_visits: 64,
                },
            )
            .unwrap();
    }
    review.select(1).unwrap();
    let held_branch = review.play(None).unwrap();
    review.select(2).unwrap();
    let peer_branch = review.play(None).unwrap();
    review.select(0).unwrap();
    review.save_profile(&profile).unwrap();
    let client = Client::spawn(db.clone(), cache.clone(), config);
    client
        .send(Command::OpenAndAnalyze(source.clone()))
        .unwrap();
    let mut engine = control(&listener);
    let foreground = query(&mut engine);
    assert_eq!(foreground["priority"], 100);
    let mut held = None;
    for _ in 0..100 {
        let q = query(&mut engine);
        let turn = q["analyzeTurns"][0].as_u64().unwrap() as usize;
        let branch = q["moves"]
            .as_array()
            .unwrap()
            .last()
            .is_some_and(|m| m[1] == "pass");
        if branch && turn == 2 {
            held = Some(q);
            continue;
        }
        finish(&mut engine, &q, turn);
        if branch && turn == 3 && q["maxVisits"].as_u64().unwrap() >= 64 {
            break;
        }
    }
    let state = until(&client, |s| {
        s.values.get(&peer_branch).is_some_and(|a| a.visits >= 64)
    });
    assert!(!state.values.contains_key(&held_branch));
    assert_eq!(
        state.values[&0].visits, 64,
        "foreground search is still held"
    );
    client.send(Command::Select(held_branch)).unwrap();
    let mut canceled = Vec::new();
    let selected = loop {
        let q = query(&mut engine);
        if q["action"] == "terminate" {
            if q["terminateId"] == held.as_ref().unwrap()["id"] {
                assert_eq!(
                    q["turnNumbers"],
                    json!([2]),
                    "use the branch turn, not its node ID"
                );
            }
            canceled.push(q["terminateId"].clone());
        } else if q["priority"] == 100 {
            break q;
        } else {
            // A previously queued background request may precede the click.
            for turn in q["analyzeTurns"].as_array().unwrap() {
                finish(&mut engine, &q, turn.as_u64().unwrap() as usize);
            }
        }
    };
    assert!(canceled.contains(&foreground["id"]));
    assert!(canceled.contains(&held.as_ref().unwrap()["id"]));
    assert!(
        canceled.len() <= 2,
        "keep unrelated background searches running"
    );
    let mut stale = held.unwrap();
    stale["maxVisits"] = json!(4096);
    finish(&mut engine, &stale, 2);
    finish(&mut engine, &selected, 2);
    let deep = loop {
        let q = query(&mut engine);
        if q["priority"] == 100 {
            break q;
        }
        for turn in q["analyzeTurns"].as_array().unwrap() {
            finish(&mut engine, &q, turn.as_u64().unwrap() as usize);
        }
    };
    assert_eq!(deep["maxVisits"], 64);
    finish(&mut engine, &deep, 2);
    let state = until(&client, |s| {
        s.values.get(&held_branch).is_some_and(|a| a.visits >= 64)
    });
    assert_eq!(state.document.as_ref().unwrap().selected, held_branch);
    assert_eq!(
        state.values[&held_branch].visits, 64,
        "canceled result must not replace the new search"
    );
    client.send(Command::Pause).unwrap();
    until(&client, |s| !s.running);
    drop(review);
    let mut reopened = Review::new(&db, &cache).unwrap();
    reopened.import(&source).unwrap();
    let saved = reopened.cached_analysis(&profile).unwrap();
    assert_eq!(saved[&held_branch].visits, 64);
    assert!(saved[&peer_branch].visits >= 64);
    assert_eq!(reopened.document().unwrap().selected, held_branch);
    assert_eq!(reopened.document().unwrap().nodes.len(), 23);
    assert_eq!(
        reopened
            .document()
            .unwrap()
            .board(held_branch)
            .unwrap()
            .stone(Point::new(0, 0)),
        Some(katastro::Color::Black)
    );
}

// Coalesced main-line queries must retain other turns when one position is
// selected; a canceled turn's late reply must not overwrite its replacement.
#[test]
fn selecting_one_turn_of_a_batch_keeps_other_results_and_persists_the_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let source = game(temp.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = config(temp.path(), &listener);
    let profile = Engine::profile(&config).unwrap();
    let db = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let mut review = Review::new(&db, &cache).unwrap();
    review.import(&source).unwrap();
    for id in 0..21 {
        review
            .store_analysis(
                &review.analysis_key(id, &profile).unwrap(),
                &Analysis {
                    visits: 64,
                    winrate: 0.5,
                    score_lead: 0.,
                    suggestions: vec![],
                    ownership: vec![0.; 81],
                    ownership_visits: 64,
                },
            )
            .unwrap();
    }
    review.save_profile(&profile).unwrap();
    let client = Client::spawn(db.clone(), cache.clone(), config);
    client
        .send(Command::OpenAndAnalyze(source.clone()))
        .unwrap();
    let mut engine = control(&listener);
    let old_foreground = query(&mut engine);
    let batch = query(&mut engine);
    let turns = batch["analyzeTurns"].as_array().unwrap();
    assert_eq!(
        turns.len(),
        7,
        "coalesce the remaining main-line slots into one query"
    );
    client.send(Command::Select(1)).unwrap();
    let selected = loop {
        let q = query(&mut engine);
        if q["action"] == "terminate" {
            if q["terminateId"] == batch["id"] {
                assert_eq!(
                    q["turnNumbers"],
                    json!([1]),
                    "cancel only the selected turn of the shared query"
                );
            } else {
                assert_eq!(q["terminateId"], old_foreground["id"]);
            }
        } else if q["priority"] == 100 {
            break q;
        } else {
            for turn in q["analyzeTurns"].as_array().unwrap() {
                finish(&mut engine, &q, turn.as_u64().unwrap() as usize);
            }
        }
    };
    let mut stale = batch.clone();
    stale["maxVisits"] = json!(4096);
    finish(&mut engine, &stale, 1);
    for turn in 2..=7 {
        finish(&mut engine, &batch, turn);
    }
    finish(&mut engine, &selected, 1);
    let state = until(&client, |s| {
        (1..=7).all(|id| s.values.get(&id).is_some_and(|a| a.visits >= 256))
    });
    assert_eq!(state.values[&1].visits, 256);
    client.send(Command::Pause).unwrap();
    until(&client, |s| !s.running);
    let mut reopened = Review::new(&db, &cache).unwrap();
    reopened.import(&source).unwrap();
    let saved = reopened.cached_analysis(&profile).unwrap();
    assert!((1..=7).all(|id| saved[&id].visits == 256));
    assert_eq!(reopened.document().unwrap().selected, 1);
    assert_eq!(reopened.document().unwrap().mainline.len(), 21);
}

// A hidden review window can stop consuming repaints for hours. Analysis must
// still finish and persist, and the first repaint must show the current game.
#[test]
fn an_unread_review_receives_the_latest_analysis_with_one_pending_repaint() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let temp = tempfile::tempdir().unwrap();
    let source = game(temp.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = config(temp.path(), &listener);
    let profile = Engine::profile(&config).unwrap();
    let db = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let client = Client::spawn_with_wake(
        db.clone(),
        cache.clone(),
        config,
        Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })),
    );
    client
        .send(Command::OpenAndAnalyze(source.clone()))
        .unwrap();
    let mut engine = control(&listener);
    loop {
        let request = query(&mut engine);
        if request["maxVisits"].as_u64().unwrap() > 64 {
            break;
        }
        for turn in request["analyzeTurns"].as_array().unwrap() {
            finish(&mut engine, &request, turn.as_u64().unwrap() as usize);
        }
    }

    // Seeing the second terminate_all on the wire means the worker has already
    // published the first pause. The window has consumed nothing yet.
    for _ in 0..2 {
        client.send(Command::Pause).unwrap();
        while query(&mut engine)["action"] != "terminate_all" {}
    }
    let pending_wakes = wakes.load(Ordering::SeqCst);
    let latest = client.recv(Duration::from_secs(5)).unwrap();
    assert_eq!(
        latest.coverage,
        (21, 21),
        "the first repaint showed an obsolete chart"
    );
    assert!(!latest.running);
    assert_eq!(
        pending_wakes, 1,
        "obsolete repaints accumulated while the window was idle"
    );
    let mut reader = Review::new(&db, &cache).unwrap();
    reader.import(&source).unwrap();
    let saved = reader.cached_analysis(&profile).unwrap();
    assert_eq!(saved.len(), 21);
    assert!(saved.values().all(|v| v.visits >= 64));

    client.send(Command::Select(7)).unwrap();
    until(&client, |s| {
        s.document.as_ref().is_some_and(|d| d.selected == 7)
    });
    client.send(Command::Pause).unwrap();
    assert_eq!(query(&mut engine)["action"], "terminate_all");
    assert!(wakes.load(Ordering::SeqCst) > pending_wakes);
}
