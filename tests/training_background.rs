use katastro::{
    engine::Engine,
    training::{Phase, Settings, Trainer},
    worker::{Client, Command, Snapshot},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    net::TcpListener,
    time::{Duration, Instant},
};
#[path = "support/controlled_engine.rs"]
mod controlled_engine;
use controlled_engine::{config, connection, query};
#[path = "support/training.rs"]
mod support;

fn until(c: &Client, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let s = c
            .recv(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(s.error.is_none(), "{:?}", s.error);
        if done(&s) {
            return s;
        }
    }
}
// Benefit: idle time produces reusable exercises, and opening a game immediately
// takes priority over discovery. Late canceled results must not alter the review.
#[test]
fn idle_discovery_yields_to_a_game_and_persists_ready_problems() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = support::fixture(root, false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = config(root, &listener);
    config.analysis.parallel_positions = 2;
    let c = Client::spawn(root.join("reviews"), root.join("cache"), config.clone());
    c.send(Command::PrepareTraining {
        games: vec![source.clone()],
        account: "me".into(),
    })
    .unwrap();
    until(&c, |s| s.preparing_training);
    let mut engine = connection(&listener);
    let held = (0..2).map(|_| query(&mut engine)).collect::<Vec<_>>();
    c.send(Command::Open(source.clone())).unwrap();
    let opened = until(&c, |s| s.document.is_some());
    assert!(!opened.preparing_training && !opened.running);
    assert_eq!(query(&mut engine)["action"], "terminate_all");
    for q in held {
        writeln!(engine.get_mut(), "{}", support::reply(&q, false, true)).unwrap();
    }
    c.send(Command::Select(1)).unwrap();
    let selected = until(&c, |s| s.document.as_ref().is_some_and(|d| d.selected == 1));
    assert_eq!(selected.board.unwrap().move_number, 1);
    drop(c);

    let c = Client::spawn(root.join("reviews"), root.join("cache"), config.clone());
    c.send(Command::PrepareTraining {
        games: vec![source.clone()],
        account: "me".into(),
    })
    .unwrap();
    until(&c, |s| s.preparing_training);
    let mut engine = connection(&listener);
    let serving = std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            if engine.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let q: Value = serde_json::from_str(&line).unwrap();
            assert_ne!(
                q["includePolicy"], true,
                "background preparation needs no HumanSL model"
            );
            writeln!(engine.get_mut(), "{}", support::reply(&q, false, true)).unwrap();
        }
    });
    until(&c, |s| {
        !s.preparing_training && s.status.contains("discovery complete")
    });
    drop(c);
    serving.join().unwrap();
    let newer = root.join("new.sgf");
    std::fs::write(
        &newer,
        std::fs::read_to_string(&source)
            .unwrap()
            .replace("KM[6.5]", "KM[7.5]"),
    )
    .unwrap();
    let mut t = Trainer::new(
        &root.join("reviews"),
        &root.join("cache"),
        &root.join("Training"),
        Engine::profile(&config).unwrap(),
    )
    .unwrap();
    t.begin(vec![newer, source], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
    t.play(Some(katastro::Point::new(3, 4))).unwrap();
    assert_eq!(
        t.next_request().unwrap().unwrap()["includePolicy"],
        json!(true)
    );
}

// Benefit: a completed review pass is immediately useful for training, without
// rerunning its evaluations. Regression: discovery only runs on an empty window.
#[test]
fn completed_review_pass_populates_training_without_extra_engine_work() {
    review_indexes_owner(false);
}
#[test]
fn completed_review_uses_archive_owner_when_opponent_matches_another_account() {
    review_indexes_owner(true);
}
fn review_indexes_owner(multiple: bool) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = support::fixture(root, false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = config(root, &listener);
    let profile = Engine::profile(&config).unwrap();
    let mut review = katastro::Review::new(&root.join("reviews"), &root.join("cache")).unwrap();
    review.import(&source).unwrap();
    for node in 0..15 {
        review
            .store_analysis(
                &review.analysis_key(node, &profile).unwrap(),
                &katastro::Analysis {
                    visits: if node == 14 { 64 } else { 1024 },
                    winrate: if node == 0 { 0.8 } else { 0.4 },
                    score_lead: if node == 0 { 8.0 } else { -2.0 },
                    suggestions: support::searched_moves(node, 64),
                    ownership: vec![0.0; 81],
                    ownership_visits: if node == 14 { 64 } else { 1024 },
                },
            )
            .unwrap();
    }
    let c = Client::spawn(root.join("reviews"), root.join("cache"), config);
    c.send(Command::OpenAndAnalyze(source.clone())).unwrap();
    let libraries = if multiple {
        vec![
            ("opponent".into(), vec![]),
            ("me".into(), vec![source.clone()]),
        ]
    } else {
        vec![("me".into(), vec![source.clone()])]
    };
    c.send(Command::PrepareTrainingLibraries(libraries))
        .unwrap();
    until(&c, |s| s.running);
    let mut engine = connection(&listener);
    let first = query(&mut engine);
    assert_eq!(
        first["maxVisits"], 4096,
        "only ordinary refinement should be requested"
    );
    let last = loop {
        let q = query(&mut engine);
        if q["analyzeTurns"].as_array().unwrap().contains(&json!(14)) {
            break q;
        }
    };
    let mut reply = json!({"id":last["id"],"turnNumber":14,"isDuringSearch":true,
        "rootInfo":{"visits":1024,"winrate":0.4,"scoreLead":-2.0}});
    writeln!(engine.get_mut(), "{reply}").unwrap();
    let partial = until(&c, |s| s.values.get(&14).is_some_and(|a| a.visits >= 1024));
    assert!(
        !partial.training_indexed,
        "unfinished search estimates must not trigger a completed-pass index"
    );
    reply["isDuringSearch"] = json!(false);
    writeln!(engine.get_mut(), "{reply}").unwrap();
    until(&c, |s| s.training_indexed);
    let newer = root.join("new.sgf");
    std::fs::write(
        &newer,
        std::fs::read_to_string(&source)
            .unwrap()
            .replace("KM[6.5]", "KM[7.5]"),
    )
    .unwrap();
    let mut t = Trainer::new(
        &root.join("reviews"),
        &root.join("cache"),
        &root.join("Training"),
        profile,
    )
    .unwrap();
    t.begin(vec![newer, source], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
    t.play(Some(katastro::Point::new(3, 4))).unwrap();
    assert_eq!(t.next_request().unwrap().unwrap()["includePolicy"], true);
}

// Benefit: idle preparation covers both servers and each player's own moves.
// Regression: the second archive replaces the first or uses its account name.
#[test]
fn idle_discovery_prepares_both_server_accounts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let panda = support::fixture(root, false);
    let fox = root.join("fox.sgf");
    std::fs::write(
        &fox,
        std::fs::read_to_string(&panda)
            .unwrap()
            .replace("PB[me]", "PB[foxplayer]")
            .replace("KM[6.5]", "KM[7.5]"),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = config(root, &listener);
    let c = Client::spawn(root.join("reviews"), root.join("cache"), config.clone());
    c.send(Command::PrepareTrainingLibraries(vec![
        ("me".into(), vec![panda.clone()]),
        ("foxplayer".into(), vec![fox.clone()]),
    ]))
    .unwrap();
    until(&c, |s| s.preparing_training);
    let mut engine = connection(&listener);
    let serving = std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            if engine.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let q: Value = serde_json::from_str(&line).unwrap();
            writeln!(engine.get_mut(), "{}", support::reply(&q, false, true)).unwrap();
        }
    });
    until(&c, |s| {
        !s.preparing_training && s.status.contains("discovery complete")
    });
    drop(c);
    serving.join().unwrap();
    for (account, source) in [("me", panda), ("foxplayer", fox)] {
        let mut trainer = Trainer::new(
            &root.join("reviews"),
            &root.join("cache"),
            &root.join("Training"),
            Engine::profile(&config).unwrap(),
        )
        .unwrap();
        trainer
            .begin(vec![source], account.into(), Settings::default())
            .unwrap();
        assert_eq!(
            trainer.snapshot().phase,
            Phase::YourTurn,
            "{account} must start immediately from the durable index"
        );
    }
}
