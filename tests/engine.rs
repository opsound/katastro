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
    engine.send(&json!({"id":"real","boardXSize":9,"boardYSize":9,"rules":"chinese","komi":7.5,"moves":[["B","D4"],["W","F6"]],"analyzeTurns":[0,1,2],"maxVisits":1})).unwrap();
    let mut turns = Vec::new();
    for _ in 0..3 {
        let value = response(&engine);
        assert_eq!(value["id"], "real");
        assert!(value.get("error").is_none(), "{value}");
        let root = &value["rootInfo"];
        assert_eq!(root["visits"], 1);
        assert!((0.0..=1.0).contains(&root["winrate"].as_f64().unwrap()));
        assert!(root["scoreLead"].as_f64().unwrap().is_finite());
        turns.push(value["turnNumber"].as_u64().unwrap());
    }
    turns.sort();
    assert_eq!(turns, vec![0, 1, 2]);
}
