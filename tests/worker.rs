use katastro::{
    Point,
    engine::EngineConfig,
    worker::{Client, Command, Snapshot},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn config(root: &Path) -> EngineConfig {
    let model = root.join("model");
    std::fs::write(&model, b"fake").unwrap();
    EngineConfig {
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model,
    }
}
fn until(client: &Client, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = client
            .recv(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if predicate(&state) {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: {} {:?}",
            state.status,
            state.error
        );
    }
}
#[test]
fn worker_keeps_variations_and_cached_chart_usable_with_engine_unavailable() {
    let temp = tempfile::TempDir::new().unwrap();
    let sgf = temp.path().join("game.sgf");
    std::fs::write(&sgf, b"(;SZ[9];B[cc];W[gg];B[cg])").unwrap();
    let config = config(temp.path());
    let db = temp.path().join("review");
    let cache = temp.path().join("cache");
    let client = Client::spawn(db.clone(), cache.clone(), config.clone());
    client.recv(Duration::from_secs(5)).unwrap();
    client.send(Command::Open(sgf.clone())).unwrap();
    let opened = client.recv(Duration::from_secs(5)).unwrap();
    assert_eq!(
        opened
            .document
            .as_ref()
            .expect("open must produce a review")
            .mainline
            .len(),
        4
    );
    client.send(Command::Start).unwrap();
    let analyzed = until(&client, |s| {
        s.coverage == (4, 4) && s.values.values().all(|v| v.visits >= 64)
    });
    assert_eq!(analyzed.values.len(), 4);
    assert!(analyzed.values.values().all(|v| v.visits >= 64));
    client.send(Command::Select(1)).unwrap();
    until(&client, |s| {
        s.document.as_ref().is_some_and(|d| d.selected == 1)
    });
    client.send(Command::Play(Some(Point::new(3, 3)))).unwrap();
    let variation = until(&client, |s| {
        s.document.as_ref().is_some_and(|d| d.nodes.len() == 5)
    });
    assert_eq!(variation.document.as_ref().unwrap().mainline.len(), 4);
    let branch = variation.document.as_ref().unwrap().selected;
    until(&client, |s| {
        s.values.get(&branch).is_some_and(|v| v.visits >= 64)
    });
    client
        .send(Command::Configure(EngineConfig {
            executable: temp.path().join("missing-engine"),
            model: config.model,
        }))
        .unwrap();
    until(&client, |s| s.config.executable.ends_with("missing-engine"));
    drop(client);
    let reopened = Client::spawn(
        db,
        cache,
        EngineConfig {
            executable: "missing".into(),
            model: "missing".into(),
        },
    );
    reopened.recv(Duration::from_secs(5)).unwrap();
    reopened.send(Command::Open(sgf)).unwrap();
    let state = until(&reopened, |s| s.document.is_some());
    assert_eq!(state.document.unwrap().nodes.len(), 5);
    assert_eq!(state.coverage, (4, 4));
    assert!(state.values[&1].visits >= 64);
    assert!(!state.running);
    reopened.send(Command::Start).unwrap();
    let failed = until(&reopened, |s| s.error.is_some());
    assert_eq!(failed.coverage, (4, 4));
    assert!(failed.values[&1].visits >= 64);
}

#[test]
fn invalid_engine_evaluation_stops_analysis_and_keeps_review_input_usable() {
    let temp = tempfile::TempDir::new().unwrap();
    let config = config(temp.path());
    std::fs::write(&config.model, b"bad-evaluation").unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc];W[gg])").unwrap();
    let client = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        config,
    );
    client.send(Command::OpenAndAnalyze(source)).unwrap();
    let failed = until(&client, |s| s.error.is_some());
    assert!(
        !failed.running,
        "invalid evaluations must not leave a permanently running search"
    );
    assert_eq!(failed.coverage, (0, 3));
    client.send(Command::Select(1)).unwrap();
    until(&client, |s| {
        s.document.as_ref().is_some_and(|d| d.selected == 1)
    });
    client.send(Command::Play(Some(Point::new(3, 3)))).unwrap();
    let variation = until(&client, |s| {
        s.document.as_ref().is_some_and(|d| d.nodes.len() == 4)
    });
    assert_eq!(variation.document.unwrap().mainline.len(), 3);
    assert!(!variation.running);
}

#[test]
fn midgame_setup_remains_reviewable_and_reports_incomplete_analysis() {
    let temp = tempfile::TempDir::new().unwrap();
    let config = config(temp.path());
    let source = temp.path().join("setup.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc];AE[cc]AB[dd];W[gg])").unwrap();
    let client = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        config,
    );
    client.send(Command::OpenAndAnalyze(source)).unwrap();
    let complete = until(&client, |s| {
        s.coverage == (2, 4) && s.values.values().all(|v| v.visits >= 64)
    });
    assert!(
        complete.coverage == (2, 4),
        "partial coverage must not claim complete analysis: {}",
        complete.status
    );
    assert!(complete.status.contains("unavailable"));
    assert!(complete.running);
    client.send(Command::Last).unwrap();
    let last = until(&client, |s| {
        s.document.as_ref().is_some_and(|d| d.selected == 3)
    });
    let board = last.board.unwrap();
    assert_eq!(board.stone(Point::new(2, 2)), None);
    assert_eq!(board.stone(Point::new(3, 3)), Some(katastro::Color::Black));
    assert_eq!(board.stone(Point::new(6, 6)), Some(katastro::Color::White));
}

#[test]
fn worker_continues_deeper_and_reports_its_current_target() {
    let temp = tempfile::TempDir::new().unwrap();
    let config = config(temp.path());
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc])").unwrap();
    let client = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        config,
    );
    client.send(Command::OpenAndAnalyze(source)).unwrap();
    let deeper = until(&client, |s| {
        s.values.values().any(|v| v.visits >= 256) || (s.coverage == (2, 2) && !s.running)
    });
    assert!(deeper.running, "automatic analysis stopped at 64 visits");
    assert!(deeper.analysis_target.is_some_and(|target| target >= 256));
    client.send(Command::Pause).unwrap();
    let paused = until(&client, |s| !s.running);
    assert!(paused.analysis_target.is_some_and(|target| target >= 256));
}
#[test]
fn streamed_deep_estimates_are_visible_but_not_cached_until_the_run_finishes() {
    let temp = tempfile::TempDir::new().unwrap();
    let config = config(temp.path());
    std::fs::write(&config.model, b"hold-final").unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc])").unwrap();
    let reviews = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let profile = katastro::engine::Engine::profile(&config).unwrap();
    let client = Client::spawn(reviews.clone(), cache.clone(), config);
    client
        .send(Command::OpenAndAnalyze(source.clone()))
        .unwrap();
    let streamed = until(&client, |s| {
        s.values.get(&0).is_some_and(|v| v.visits >= 64)
    });
    assert_eq!(streamed.values[&0].score_lead, 2.5);
    client.send(Command::Pause).unwrap();
    until(&client, |s| !s.running);
    drop(client);
    let mut review = katastro::Review::new(&reviews, &cache).unwrap();
    review.import(&source).unwrap();
    assert!(
        review.cached_analysis(&profile).unwrap().is_empty(),
        "an unfinished streamed run was cached"
    );
}

#[test]
fn pause_and_resume_preserve_transient_estimates_in_the_open_review() {
    let temp = tempfile::TempDir::new().unwrap();
    let config = config(temp.path());
    std::fs::write(&config.model, b"hold-final").unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc])").unwrap();
    let client = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        config,
    );
    client.send(Command::OpenAndAnalyze(source)).unwrap();
    until(&client, |s| {
        s.values.get(&0).is_some_and(|v| v.visits >= 64)
    });
    client.send(Command::Pause).unwrap();
    let paused = until(&client, |s| !s.running);
    client.send(Command::Start).unwrap();
    let resumed = until(&client, |s| s.status.starts_with("Analyzing"));
    assert_eq!(
        resumed.values, paused.values,
        "resuming discarded uncached chart estimates"
    );
}
