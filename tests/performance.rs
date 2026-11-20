use katastro::{
    Document, Review,
    engine::EngineConfig,
    worker::{Client, Command, Snapshot},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn until(client: &Client, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let state = client
            .recv(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(state.error.is_none(), "{}: {:?}", state.status, state.error);
        if predicate(&state) {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "{}: {:?}",
            state.status,
            state.coverage
        );
    }
}

#[test]
#[ignore = "requires a local Metal KataGo binary, model, and private SGF corpus"]
fn live_worker_chart_and_cached_reopen_measurements() {
    let config = EngineConfig {
        analysis: Default::default(),
        executable: std::env::var_os("KATASTRO_TEST_ENGINE")
            .expect("KATASTRO_TEST_ENGINE")
            .into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL")
            .expect("KATASTRO_TEST_MODEL")
            .into(),
    };
    let corpus = PathBuf::from(std::env::var_os("KATASTRO_SGF_DIR").expect("KATASTRO_SGF_DIR"));
    let mut games: Vec<_> = std::fs::read_dir(corpus)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sgf"))
        .map(|path| {
            let doc = Document::parse(&std::fs::read(&path).unwrap()).unwrap();
            (doc.mainline.len(), path)
        })
        .collect();
    games.sort();
    assert!(games.len() >= 2);
    let sample = [&games[0], &games[games.len() / 2]];
    for trial in 1..=3 {
        let temp = tempfile::TempDir::new().unwrap();
        let client = Client::spawn(
            temp.path().join("reviews"),
            temp.path().join("cache"),
            config.clone(),
        );
        client.recv(Duration::from_secs(5)).unwrap();
        for (index, (positions, path)) in sample.iter().enumerate() {
            let start = Instant::now();
            client.send(Command::OpenAndAnalyze(path.clone())).unwrap();
            let first = until(&client, |s| {
                s.coverage.0 > 0 && s.source.as_ref() == Some(path)
            });
            let first_elapsed = start.elapsed();
            assert!(first.values.values().all(|v| v.visits > 0));
            let covered = if first.coverage == (*positions, *positions) {
                first
            } else {
                until(&client, |s| s.coverage == (*positions, *positions))
            };
            let chart_elapsed = start.elapsed();
            // Cheap chart values remain transient; finish one selected deep run before checking cache reuse.
            let mut reader =
                Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
            reader.import(path).unwrap();
            let profile = katastro::engine::Engine::profile(&config).unwrap();
            let selected = *covered.document.as_ref().unwrap().mainline.last().unwrap();
            client.send(Command::Select(selected)).unwrap();
            until(&client, |_| {
                reader
                    .cached_analysis(&profile)
                    .unwrap()
                    .get(&selected)
                    .is_some_and(|v| v.visits >= 64)
            });
            client.send(Command::Pause).unwrap();
            until(&client, |s| !s.running);
            let saved = reader.cached_analysis(&profile).unwrap();
            assert!(saved.values().all(|v| v.visits >= 64));
            let reopen = Instant::now();
            client.send(Command::Open(path.clone())).unwrap();
            let restored = until(&client, |s| s.status.starts_with("Review ready"));
            let reopen_elapsed = reopen.elapsed();
            assert_eq!(restored.values, saved);
            assert!(restored.values.contains_key(&selected));
            println!(
                "trial={trial} {} positions={positions} first={:.3}s full_chart={:.3}s deep_cached_reopen={:.3}s cached_positions={}",
                if index == 0 { "cold" } else { "warm" },
                first_elapsed.as_secs_f64(),
                chart_elapsed.as_secs_f64(),
                reopen_elapsed.as_secs_f64(),
                restored.values.len()
            );
        }
    }
}

#[test]
#[ignore = "requires a local engine, model, and representative SGF via KATASTRO_TEST_SGF"]
fn warm_resume_to_worker_snapshot() {
    use katastro::{Analysis, engine::Engine};
    let path = PathBuf::from(std::env::var_os("KATASTRO_TEST_SGF").unwrap());
    let mut config = EngineConfig {
        executable: std::env::var_os("KATASTRO_TEST_ENGINE").unwrap().into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL").unwrap().into(),
        analysis: Default::default(),
    };
    config.analysis.max_visits = Some(64);
    let temp = tempfile::TempDir::new().unwrap();
    let db = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let mut review = Review::new(&db, &cache).unwrap();
    review.import(&path).unwrap();
    let doc = review.document().unwrap();
    let nodes = doc.nodes.len();
    let value = Analysis {
        visits: 64,
        score_lead: 0.0,
        winrate: 0.5,
        suggestions: vec![],
        ownership: vec![0.0; doc.size * doc.size],
        ownership_visits: 64,
    };
    let profile = Engine::profile(&config).unwrap();
    for id in 0..nodes {
        let key = review.analysis_key(id, &profile).unwrap();
        review.store_analysis(&key, &value).unwrap();
    }
    review.save_profile(&profile).unwrap();
    drop(review);
    let client = Client::spawn(db, cache, config);
    client.send(Command::OpenAndAnalyze(path)).unwrap();
    until(&client, |s| s.running);
    let mut samples = Vec::new();
    for _ in 0..7 {
        client.send(Command::Pause).unwrap();
        until(&client, |s| s.status.starts_with("Analysis paused"));
        let start = Instant::now();
        client.send(Command::Start).unwrap();
        let result = until(&client, |s| s.running);
        samples.push(start.elapsed());
        assert_eq!(result.values.len(), nodes);
    }
    samples.sort();
    println!(
        "positions={nodes} median warm resume={:.2} ms",
        samples[3].as_secs_f64() * 1000.
    );
}

// Historical backlog measurement. Run this test alone so process footprint
// reflects a single reviewer. Only this opt-in measurement uses elapsed time.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "measures an unread review with real KataGo and a representative SGF on macOS"]
fn live_unattended_review_memory() {
    use std::process::Command as Process;
    let source =
        PathBuf::from(std::env::var_os("KATASTRO_MEMORY_SGF").expect("KATASTRO_MEMORY_SGF"));
    let temp = tempfile::tempdir().unwrap();
    let cfg = EngineConfig::discover(temp.path());
    let client = Client::spawn(temp.path().join("reviews"), temp.path().join("cache"), cfg);
    client.send(Command::OpenAndAnalyze(source)).unwrap();
    let warm = until(&client, |s| {
        s.coverage.0 > 0 && s.coverage.0 == s.coverage.1
    });
    println!("positions={}", warm.coverage.0);
    drop(warm);
    let pid = std::process::id().to_string();
    let child = Process::new("/usr/bin/pgrep")
        .args(["-P", &pid, "-x", "katago"])
        .output()
        .unwrap();
    assert!(child.status.success());
    let child = String::from_utf8(child.stdout).unwrap();
    let footprint = |pid: &str| {
        let out = Process::new("/usr/bin/vmmap")
            .args(["-summary", pid])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let report = String::from_utf8(out.stdout).unwrap();
        report
            .lines()
            .find_map(|line| line.strip_prefix("Physical footprint:"))
            .unwrap()
            .trim()
            .to_owned()
    };
    let start = Instant::now();
    for secs in [0, 10, 20, 30] {
        std::thread::sleep(Duration::from_secs(secs).saturating_sub(start.elapsed()));
        println!(
            "unread_s={secs} reviewer={} katago={}",
            footprint(&pid),
            footprint(child.trim())
        );
    }
    let mut unread = 0;
    while client.snapshots.try_recv().is_ok() {
        unread += 1;
    }
    println!("unread_snapshots={unread}");
    client.send(Command::Pause).unwrap();
    until(&client, |s| !s.running && s.coverage.0 == s.coverage.1);
}
