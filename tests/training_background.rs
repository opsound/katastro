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
// A sync updates the work list while the user has the library engine paused.
#[test]
fn manual_pause_holds_new_library_work_until_resume() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let old = support::fixture(root, false);
    let new = root.join("new.sgf");
    std::fs::write(
        &new,
        std::fs::read_to_string(&old)
            .unwrap()
            .replace("KM[6.5]", "KM[7.5]"),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = config(root, &listener);
    config.analysis.parallel_positions = 1;
    let c = Client::spawn(root.join("reviews"), root.join("cache"), config);
    c.send(Command::PrepareTrainingLibraries(vec![(
        "me".into(),
        vec![old],
    )]))
    .unwrap();
    until(&c, |s| s.preparing_training);
    let mut engine = connection(&listener);
    assert_eq!(query(&mut engine)["komi"], 6.5);
    c.send(Command::Pause).unwrap();
    until(&c, |s| !s.preparing_training && s.status.contains("paused"));
    assert_eq!(query(&mut engine)["action"], "terminate_all");
    c.send(Command::PrepareTrainingLibraries(vec![(
        "me".into(),
        vec![new],
    )]))
    .unwrap();
    // Saving a different setting acknowledges a command after the library refresh.
    let settings = katastro::archive::StatisticsSettings {
        compare: false,
        ..Default::default()
    };
    c.send(Command::SaveGameStatistics(settings)).unwrap();
    let refreshed = until(&c, |s| s.game_statistics == Some(settings));
    assert!(
        !refreshed.preparing_training,
        "a sync resumed the engine during a manual pause"
    );
    c.send(Command::Start).unwrap();
    until(&c, |s| s.preparing_training);
    assert_eq!(
        query(&mut engine)["komi"],
        7.5,
        "resuming must use the refreshed library"
    );
}

#[test]
fn browsing_games_can_screen_the_library_while_a_review_stays_open() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let library = support::fixture(root, false);
    let review = root.join("review.sgf");
    std::fs::write(&review, "(;SZ[9]KM[7.5];B[bb];W[cc])").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = config(root, &listener);
    config.analysis.parallel_positions = 1;
    let c = Client::spawn(root.join("reviews"), root.join("cache"), config);
    c.send(Command::Open(review)).unwrap();
    c.send(Command::Select(2)).unwrap();
    c.send(Command::PrepareTrainingLibraries(vec![(
        "me".into(),
        vec![library],
    )]))
    .unwrap();
    c.send(Command::BrowseGames(true)).unwrap();
    let browsing = until(&c, |s| s.preparing_training);
    assert_eq!(browsing.document.unwrap().selected, 2);
    assert!(!browsing.running);
    let mut engine = connection(&listener);
    let held = query(&mut engine);
    assert_eq!(held["komi"], 6.5);
    c.send(Command::BrowseGames(false)).unwrap();
    until(&c, |s| !s.preparing_training);
    assert_eq!(query(&mut engine)["action"], "terminate_all");
    writeln!(engine.get_mut(), "{}", support::reply(&held, false, true)).unwrap();
    c.send(Command::Start).unwrap();
    let active = until(&c, |s| s.running);
    assert!(!active.preparing_training && active.values.is_empty());
    assert_eq!(active.document.unwrap().selected, 2);
    assert_eq!(query(&mut engine)["komi"], 7.5);
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

// All synced games should contribute to the Overview before background training
// starts its more expensive confirmations. A server-at-a-time trainer starves later games.
#[test]
fn idle_shallow_analysis_classifies_both_servers_before_training_confirmation() {
    use katastro::{
        Review,
        archive::{
            Game,
            categories::{Category, classify},
        },
    };
    use std::collections::HashSet;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let panda = support::fixture(root, false);
    let sgf = std::fs::read_to_string(&panda).unwrap();
    std::fs::write(&panda, sgf.replace("PB[me]", "PB[me]RE[B+R]")).unwrap();
    let fox = root.join("fox.sgf");
    std::fs::write(
        &fox,
        sgf.replace("PB[me]", "PB[foxplayer]RE[W+R]")
            .replace(";B[aa]", ";B[ii]"),
    )
    .unwrap();
    let indexed = root.join("older.sgf");
    std::fs::write(&indexed, "(;SZ[9]KM[7.5];B[hh];W[gg])").unwrap();
    let libraries = vec![
        ("me".into(), vec![panda.clone(), indexed.clone()]),
        ("foxplayer".into(), vec![fox.clone()]),
    ];
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = config(root, &listener);
    config.analysis.parallel_positions = 1;
    let profile = Engine::profile(&config).unwrap();
    let c = Client::spawn(root.join("reviews"), root.join("cache"), config.clone());
    c.send(Command::PrepareTrainingLibraries(libraries.clone()))
        .unwrap();
    until(&c, |s| s.preparing_training);
    let mut engine = connection(&listener);
    let answer = |q: &Value| {
        let mut reply = support::reply(q, false, true);
        reply["rootInfo"]["scoreLead"] = json!(if q["komi"] == 7.5 {
            0
        } else {
            match q["moves"].as_array().unwrap().len() {
                0 => 0,
                1 => -24,
                _ => 22,
            }
        });
        reply
    };
    let mut screened = HashSet::new();
    let mut opening_order = Vec::new();
    let first_confirmation = loop {
        let q = query(&mut engine);
        if q["maxVisits"] != 64 {
            break q;
        }
        if q["moves"].as_array().unwrap().len() == 1 {
            opening_order.push(q["komi"].as_f64().unwrap());
        }
        let key = (q["komi"].to_string(), q["moves"].to_string());
        assert!(screened.insert(key), "shared opening was screened twice");
        writeln!(engine.get_mut(), "{}", answer(&q)).unwrap();
    };
    assert_eq!(first_confirmation["maxVisits"], 512);
    let review = Review::new(&root.join("reviews"), &root.join("cache")).unwrap();
    for (source, account, expected) in [
        (&panda, "me", Category::Comeback),
        (&fox, "foxplayer", Category::Blunder),
        (&indexed, "me", Category::Other),
    ] {
        let game = Game {
            id: String::new(),
            timestamp: None,
            date: String::new(),
            result: "B+R".into(),
            white: "opponent".into(),
            black: account.into(),
            path: source.clone(),
        };
        assert_eq!(
            classify(&review, &game, account, Some(&profile)).unwrap(),
            expected,
            "the Overview should already classify {} before the first deep query",
            source.display()
        );
    }
    assert_eq!(
        review.last_profile().unwrap().as_ref(),
        Some(&profile),
        "a fresh offline Overview needs the background profile before any manual review"
    );
    assert_eq!(
        opening_order,
        [6.5, 6.5, 7.5],
        "both servers should receive a turn before the second game on one server"
    );
    // Two 15-position games share the opening position; the older 3-position game has different komi.
    assert_eq!(screened.len(), 15 + 14 + 3);
    writeln!(engine.get_mut(), "{}", answer(&first_confirmation)).unwrap();
    let serving = std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            if engine.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let q: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(q["maxVisits"], 512, "training repeated a shallow screen");
            writeln!(engine.get_mut(), "{}", answer(&q)).unwrap();
        }
    });
    let complete = |s: &Snapshot| !s.preparing_training && s.status.contains("discovery complete");
    until(&c, complete);
    drop(c);
    serving.join().unwrap();
    let reopened = Client::spawn(root.join("reviews"), root.join("cache"), config);
    reopened
        .send(Command::PrepareTrainingLibraries(libraries))
        .unwrap();
    until(&reopened, complete);
    drop(reopened);
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "reopening a prepared library should use its cache without starting an engine"
    );
    for (account, source) in [("me", panda), ("foxplayer", fox)] {
        let mut trainer = Trainer::new(
            &root.join("reviews"),
            &root.join("cache"),
            &root.join("Training"),
            profile.clone(),
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

// A newly synced game can start contributing to statistics during an older game's confirmation.
#[test]
fn newly_synced_game_preempts_training_confirmation_without_accepting_its_stale_reply() {
    use katastro::{Review, archive::StatisticsSettings};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let old = support::fixture(root, false);
    let new = root.join("new.sgf");
    let sgf = std::fs::read_to_string(&old).unwrap();
    std::fs::write(&new, sgf.replace("KM[6.5]", "KM[7.5]")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = config(root, &listener);
    config.analysis.parallel_positions = 1;
    let profile = Engine::profile(&config).unwrap();
    let mut review = Review::new(&root.join("reviews"), &root.join("cache")).unwrap();
    review.import(&old).unwrap();
    let c = Client::spawn(root.join("reviews"), root.join("cache"), config);
    let prepare = |games| {
        c.send(Command::PrepareTrainingLibraries(vec![(
            "me".into(),
            games,
        )]))
        .unwrap()
    };
    prepare(vec![old.clone()]);
    until(&c, |s| s.preparing_training);
    let mut engine = connection(&listener);
    let held = loop {
        let q = query(&mut engine);
        if q["maxVisits"] == 512 {
            break q;
        }
        writeln!(engine.get_mut(), "{}", support::reply(&q, false, true)).unwrap();
    };
    prepare(vec![new, old]);
    let saved = StatisticsSettings {
        compare: false,
        ..Default::default()
    };
    c.send(Command::SaveGameStatistics(saved)).unwrap();
    until(&c, |s| s.game_statistics == Some(saved));
    // Deliver a terminated search as if KataGo finished sending it during cancellation.
    writeln!(engine.get_mut(), "{}", support::reply(&held, false, true)).unwrap();
    assert_eq!(
        query(&mut engine)["action"],
        "terminate_all",
        "newly synced games must preempt deep training confirmation"
    );
    let next = query(&mut engine);
    assert_eq!(
        (&next["komi"], &next["maxVisits"]),
        (&json!(7.5), &json!(64))
    );
    writeln!(engine.get_mut(), "{}", support::reply(&next, false, true)).unwrap();
    assert_eq!(query(&mut engine)["komi"], 7.5);
    let node = held["analyzeTurns"][0].as_u64().unwrap() as usize;
    assert_eq!(
        review.cached_analysis(&profile).unwrap()[&node].visits,
        64,
        "the cancelled confirmation was accepted into the permanent cache"
    );
}
