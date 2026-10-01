use katastro::engine::{Engine, EngineConfig, EngineEvent};
use serde_json::json;
use std::time::{Duration, Instant};
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
