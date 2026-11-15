use katastro::{
    engine::{Engine, EngineConfig, EngineEvent},
    training::{Phase, Settings, Trainer},
};
use serde_json::json;
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires KATASTRO_TRAINING_LIBRARY, KATASTRO_TEST_ENGINE and KATASTRO_TEST_MODEL; reads private local games"]
fn real_background_preparation_opens_a_cached_problem_without_new_discovery() {
    let library = std::path::PathBuf::from(std::env::var_os("KATASTRO_TRAINING_LIBRARY").unwrap());
    let db = rusqlite::Connection::open_with_flags(
        library.join("sync.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let account: String = db
        .query_row("SELECT username FROM config LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let games: Vec<_> = db
        .prepare("SELECT filename FROM games WHERE username=?1 ORDER BY date DESC LIMIT 5")
        .unwrap()
        .query_map([&account], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| library.join(r.unwrap()))
        .collect();
    let hashes: Vec<_> = games
        .iter()
        .map(|p| katastro::digest(&std::fs::read(p).unwrap()))
        .collect();
    let config = EngineConfig {
        analysis: Default::default(),
        executable: std::env::var_os("KATASTRO_TEST_ENGINE").unwrap().into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL").unwrap().into(),
    };
    let profile = Engine::profile(&config).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let new_trainer = || {
        Trainer::new(
            &root.join("reviews"),
            &root.join("cache"),
            &root.join("Training"),
            profile.clone(),
        )
        .unwrap()
    };
    let started = Instant::now();
    let mut background = new_trainer();
    background
        .prepare(games.clone(), account.clone(), false)
        .unwrap();
    let mut engine = Engine::spawn(&config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        while let Some(q) = background.next_request().unwrap() {
            engine.send(&q).unwrap();
        }
        let event = engine
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if let EngineEvent::Response(reply) = event {
            background.accept(&reply, 0.5).unwrap();
            if reply["rootInfo"]["visits"].as_u64().unwrap_or(0) >= 512 {
                let mut ready = new_trainer();
                let opening = Instant::now();
                ready
                    .begin(games.clone(), account.clone(), Settings::default())
                    .unwrap();
                let elapsed = opening.elapsed();
                if ready.snapshot().phase == Phase::YourTurn {
                    if let Some(q) = ready.next_request().unwrap() {
                        assert_eq!(
                            q["maxVisits"], 512,
                            "only the comparison window may need preparation; never restart discovery"
                        );
                    }
                    let d = ready.snapshot().document.unwrap();
                    let point = (0..d.size * d.size)
                        .map(|i| Some(katastro::Point::new(i % d.size, i / d.size)))
                        .find(|p| d.clone().append_move(*p).is_ok())
                        .unwrap_or(None);
                    ready.play(point).unwrap();
                    assert_eq!(
                        ready.next_request().unwrap().unwrap()["includePolicy"],
                        true
                    );
                    eprintln!(
                        "Training preparation timing: {}",
                        json!({"first_ready_ms":started.elapsed().as_millis(),"cached_start_ms":elapsed.as_millis(),"board_size":d.size})
                    );
                    break;
                }
            }
        }
        assert!(Instant::now() < deadline, "no ready exercise found");
    }
    drop(engine);
    for (p, hash) in games.iter().zip(hashes) {
        assert_eq!(katastro::digest(&std::fs::read(p).unwrap()), hash);
    }
}
