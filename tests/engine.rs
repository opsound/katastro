use katastro::engine::{Engine, EngineConfig, EngineEvent};
use serde_json::json;
use std::time::{Duration, Instant};

#[test]
#[ignore = "Requires KATASTRO_TEST_ENGINE and KATASTRO_TEST_MODEL"]
fn live_library_screen_populates_review_and_statistics_from_a_synced_original() {
    use katastro::{
        Review,
        archive::{
            Game,
            categories::{Category, Thresholds, evidence},
        },
        worker::{Client, Command},
    };
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, "(;FF[4]GM[1]SZ[9]KM[6.5]RE[W+R];B[dd];W[ff])").unwrap();
    let config = EngineConfig {
        executable: std::env::var_os("KATASTRO_TEST_ENGINE").unwrap().into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL").unwrap().into(),
        analysis: Default::default(),
    };
    let reviews = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let c = Client::spawn(reviews.clone(), cache.clone(), config);
    c.send(Command::PrepareTrainingLibraries(vec![(
        "member".into(),
        vec![source.clone()],
    )]))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let snapshot = c
            .recv(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
        assert!(snapshot.document.is_none());
        if snapshot.status.contains("discovery complete") {
            break;
        }
    }
    drop(c);
    let mut reopened = Review::new(&reviews, &cache).unwrap();
    let profile = reopened.last_profile().unwrap().unwrap();
    let game = Game {
        id: "1".into(),
        date: "2026-01-01".into(),
        timestamp: None,
        white: "member".into(),
        black: "other".into(),
        result: "W+R".into(),
        path: source.clone(),
    };
    let outcome = evidence(&reopened, &game, "member", Some(&profile)).unwrap();
    assert_eq!(outcome.won(), Some(true));
    assert_eq!(
        outcome.category(Thresholds {
            comeback: 999.,
            blunder: 999.
        }),
        Category::Other
    );
    reopened.import(&source).unwrap();
    let scores = reopened.cached_analysis(&profile).unwrap();
    assert_eq!(scores.len(), 3);
    assert!(scores.values().all(|score| score.visits >= 64));
}

fn response(engine: &Engine) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        assert!(Instant::now() < deadline);
        match engine.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => match event {
                EngineEvent::Response(value) => return value,
                EngineEvent::Diagnostic(_) => {}
                EngineEvent::Fault(error) => panic!("{error}"),
                EngineEvent::Exited => panic!("unexpected EOF"),
            },
            Err(error)
                if error.downcast_ref::<std::sync::mpsc::RecvTimeoutError>()
                    == Some(&std::sync::mpsc::RecvTimeoutError::Timeout) =>
            {
                continue;
            }
            Err(error) => panic!("{error}"),
        }
    }
}
#[test]
fn real_pipes_carry_reverse_order_streaming_results_and_cancellation_ack() {
    let temp = tempfile::TempDir::new().unwrap();
    let model = temp.path().join("model.bin");
    std::fs::write(&model, b"fake model").unwrap();
    let config = EngineConfig {
        analysis: Default::default(),
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model,
    };
    let profile = Engine::profile(&config).unwrap();
    assert!(!profile.model_sha256.is_empty());
    let mut engine = Engine::spawn(&config).unwrap();
    engine.send(&json!({"id":"query","analyzeTurns":[0,1,2],"maxVisits":8,"reportDuringSearchEvery":0.2})).unwrap();
    let mut turns = Vec::new();
    for _ in 0..3 {
        let partial = response(&engine);
        assert_eq!(partial["isDuringSearch"], true);
        let final_reply = response(&engine);
        assert_eq!(final_reply["isDuringSearch"], false);
        turns.push(final_reply["turnNumber"].as_u64().unwrap());
    }
    assert_eq!(turns, vec![2, 1, 0]);
    engine
        .send(&json!({"id":"cancel","action":"terminate","terminateId":"query"}))
        .unwrap();
    assert_eq!(response(&engine)["terminateId"], "query");
    engine.send(&json!({"id":"held","action":"hold","query":{"id":"later","analyzeTurns":[3],"maxVisits":1}})).unwrap();
    assert_eq!(response(&engine)["action"], "held");
    engine.send(&json!({"action":"release"})).unwrap();
    assert_eq!(response(&engine)["id"], "later");
    engine.send(&json!({"action":"crash"})).unwrap();
    loop {
        if matches!(
            engine.recv_timeout(Duration::from_secs(1)).unwrap(),
            EngineEvent::Exited
        ) {
            break;
        }
    }
}
#[test]
#[ignore = "Requires explicit local KataGo executable and model; run with --ignored"]
fn live_katago_protocol_returns_real_estimates_when_explicitly_configured() {
    let executable =
        std::env::var("KATASTRO_TEST_ENGINE").expect("KATASTRO_TEST_ENGINE is required");
    let model = std::env::var("KATASTRO_TEST_MODEL").expect("KATASTRO_TEST_MODEL is required");
    let mut engine = Engine::spawn(&EngineConfig {
        analysis: Default::default(),
        executable: executable.into(),
        model: model.into(),
    })
    .unwrap();
    engine.send(&json!({"id":"real","boardXSize":9,"boardYSize":9,"rules":"chinese","komi":7.5,"moves":[["B","D4"],["W","F6"]],"analyzeTurns":[0,1,2],"maxVisits":1,"includeOwnership":true})).unwrap();
    let mut turns = Vec::new();
    for _ in 0..3 {
        let value = response(&engine);
        assert_eq!(value["id"], "real");
        assert!(value.get("error").is_none(), "{value}");
        let root = &value["rootInfo"];
        assert_eq!(root["visits"], 1);
        assert!((0.0..=1.0).contains(&root["winrate"].as_f64().unwrap()));
        assert!(root["scoreLead"].as_f64().unwrap().is_finite());
        let ownership: Vec<f64> = serde_json::from_value(value["ownership"].clone()).unwrap();
        assert_eq!(
            ownership.len(),
            81,
            "even a one-visit selected-position request must return a usable map"
        );
        assert!(ownership.iter().all(|v| (-1.0..=1.0).contains(v)));
        turns.push(value["turnNumber"].as_u64().unwrap());
    }
    turns.sort();
    assert_eq!(turns, vec![0, 1, 2]);
}

#[test]
#[ignore = "Requires explicit local KataGo executable and model; run with --ignored"]
fn live_scheduler_refines_to_256_and_round_trips_ranked_suggestions() {
    use katastro::{Document, Review, scheduler::Scheduler};
    let config = EngineConfig {
        analysis: Default::default(),
        executable: std::env::var_os("KATASTRO_TEST_ENGINE")
            .expect("KATASTRO_TEST_ENGINE")
            .into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL")
            .expect("KATASTRO_TEST_MODEL")
            .into(),
    };
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    let sgf = b"(;SZ[9];B[df];W[fd])";
    std::fs::write(&source, sgf).unwrap();
    let profile = Engine::profile(&config).unwrap();
    let mut engine = Engine::spawn(&config).unwrap();
    let mut scheduler = Scheduler::new(
        1,
        Document::parse(sgf).unwrap(),
        profile.clone(),
        Default::default(),
    );
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    for budget in [1, 8, 64, 256] {
        while scheduler.values.len() < 3 || scheduler.values.values().any(|a| a.visits < budget) {
            let request = scheduler.next_request().unwrap().unwrap();
            assert_eq!(request.visits, budget);
            engine.send(&request.query).unwrap();
            while scheduler.pending() > 0 {
                let reply = response(&engine);
                for (target, value) in scheduler.accept(&reply).unwrap() {
                    let searched = reply["moveInfos"].as_array().map_or(0, Vec::len);
                    if searched > 0 {
                        assert_eq!(
                            value.suggestions.len(),
                            searched,
                            "the real engine's candidate list was truncated"
                        );
                    } else if let Some(policy) = reply["policy"].as_array() {
                        assert_eq!(
                            value.suggestions.len(),
                            policy
                                .iter()
                                .filter(|p| p.as_f64().is_some_and(|p| p >= 0.0))
                                .count()
                        );
                    }
                    if budget > 1 {
                        let ownership: Vec<f64> =
                            serde_json::from_value(reply["ownership"].clone()).unwrap();
                        assert_eq!(ownership.len(), 81);
                        assert_eq!(value.ownership, ownership);
                        assert!(ownership.iter().all(|v| (-1.0..=1.0).contains(v)));
                    } else {
                        assert!(value.ownership.is_empty());
                    }
                    if reply["isDuringSearch"] == false {
                        review.store_analysis(&target.key, &value).unwrap();
                    }
                }
            }
        }
        assert!(scheduler.values.values().all(|v| v.visits >= budget));
        assert!(scheduler.values.values().all(|v| !v.suggestions.is_empty()));
        assert!(!scheduler.is_complete());
    }
    assert!(
        scheduler.values.values().any(|v| v.suggestions.len() > 5),
        "the real test must exercise candidates beyond five"
    );
    let counts: Vec<_> = scheduler
        .values
        .values()
        .map(|v| v.suggestions.len())
        .collect();
    drop(review);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&source).unwrap();
    assert_eq!(
        reopened.cached_analysis(&profile).unwrap(),
        scheduler.values
    );
    println!(
        "Real KataGo: three positions refined to 256 visits, complete candidate counts {counts:?}, exact cached round trip"
    );
}

#[test]
#[ignore = "Requires explicit local KataGo executable and model; run with --ignored"]
fn live_partial_cancellation_preserves_other_turns_of_a_shared_query() {
    let mut engine = Engine::spawn(&EngineConfig {
        analysis: Default::default(),
        executable: std::env::var_os("KATASTRO_TEST_ENGINE")
            .expect("engine")
            .into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL")
            .expect("model")
            .into(),
    })
    .unwrap();
    engine
        .send(
            &json!({"id":"batch","boardXSize":9,"boardYSize":9,"rules":"chinese",
        "komi":7.5,"moves":[["B","D4"],["W","F6"]],"analyzeTurns":[0,1,2],"maxVisits":64}),
        )
        .unwrap();
    engine
        .send(&json!({"id":"cancel","action":"terminate","terminateId":"batch","turnNumbers":[1]}))
        .unwrap();
    let mut completed = std::collections::BTreeSet::new();
    let mut ack = false;
    while completed.len() < 3 || !ack {
        let result = response(&engine);
        assert!(result.get("error").is_none(), "{result}");
        if result["action"] == "terminate" {
            ack = true;
            continue;
        }
        assert_eq!(result["id"], "batch");
        if result["isDuringSearch"] == false {
            let turn = result["turnNumber"].as_u64().unwrap();
            if turn != 1 {
                assert!(result["rootInfo"]["visits"].as_u64().unwrap() >= 64);
            }
            completed.insert(turn);
        }
    }
    assert_eq!(completed, std::collections::BTreeSet::from([0, 1, 2]));
}

// KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago KATASTRO_TEST_MODEL=...
// KATASTRO_TEST_HUMAN_MODEL=... cargo test --test engine real_human_policy -- --ignored
#[test]
#[ignore = "requires KataGo >=1.15, normal model and b18c384nbt-humanv0 model"]
fn real_human_policy_and_strong_multi_move_lines_share_a_persistent_engine() {
    let config = EngineConfig {
        analysis: Default::default(),
        executable: std::env::var_os("KATASTRO_TEST_ENGINE")
            .expect("engine")
            .into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL")
            .expect("model")
            .into(),
    };
    let human = std::path::PathBuf::from(
        std::env::var_os("KATASTRO_TEST_HUMAN_MODEL").expect("human model"),
    );
    let mut engine = Engine::spawn_with_human(&config, &human).unwrap();
    let doc = katastro::Document::parse(b"(;SZ[9]KM[6.5];B[cc];W[gg])").unwrap();
    let mut q = serde_json::to_value(doc.position(2).unwrap()).unwrap();
    q["id"] = json!("human");
    q["maxVisits"] = json!(1);
    q["includePolicy"] = json!(true);
    q["overrideSettings"] = json!({"humanSLProfile":"rank_1d"});
    engine.send(&q).unwrap();
    let r = response(&engine);
    assert!(r.get("error").is_none(), "{r}");
    let policy = r["humanPolicy"]
        .as_array()
        .expect("a rank-1d opponent needs HumanSL probabilities");
    assert_eq!(policy.len(), 82);
    assert!(policy.iter().filter(|p| p.as_f64().unwrap() > 0.0).count() > 2);
    assert!(
        policy[2 * 9 + 2].as_f64().unwrap() < 0.0,
        "occupied points cannot be played"
    );
    q["id"] = json!("strong");
    q["maxVisits"] = json!(512);
    q["includePolicy"] = json!(false);
    q["overrideSettings"] = json!({"analysisPVLen":12});
    engine.send(&q).unwrap();
    let r = response(&engine);
    assert!(r.get("error").is_none(), "{r}");
    assert!(r["rootInfo"]["visits"].as_u64().unwrap() >= 512);
    let pv = r["moveInfos"][0]["pv"].as_array().unwrap();
    assert!(pv.len() >= 2);
    let mut line = doc.clone();
    line.selected = 2;
    for m in pv {
        line.append_move(katastro::Point::from_gtp(m.as_str().unwrap(), line.size).unwrap())
            .unwrap();
    }
}

#[test]
#[ignore = "Requires explicit local KataGo executable and model; run with --ignored"]
fn live_custom_analysis_settings_stop_at_the_requested_visit_limit() {
    use katastro::{Document, engine::AnalysisSettings, scheduler::Scheduler};
    let config = EngineConfig {
        executable: std::env::var_os("KATASTRO_TEST_ENGINE")
            .expect("engine")
            .into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL")
            .expect("model")
            .into(),
        analysis: AnalysisSettings {
            max_visits: Some(100),
            parallel_positions: 2,
            threads_per_position: 2,
        },
    };
    let mut engine = Engine::spawn(&config).unwrap();
    let mut s = Scheduler::new(
        1,
        Document::parse(b"(;SZ[9];B[cc];W[gg])").unwrap(),
        Engine::profile(&config).unwrap(),
        Default::default(),
    )
    .with_settings(&config.analysis)
    .unwrap();
    while !s.is_complete() {
        while let Some(q) = s.next_request().unwrap() {
            assert!(q.visits <= 100);
            engine.send(&q.query).unwrap();
        }
        assert!(s.pending() > 0, "unfinished analysis must make progress");
        s.accept(&response(&engine)).unwrap();
    }
    assert_eq!(s.coverage(), (3, 3));
    assert!(
        s.values
            .values()
            .all(|a| a.visits >= 100 && a.ownership.len() == 81)
    );
    assert!(s.next_request().unwrap().is_none());
}
