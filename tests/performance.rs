use katastro::{
    Document,
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
            client.send(Command::Pause).unwrap();
            until(&client, |s| !s.running);
            let reopen = Instant::now();
            client.send(Command::Open(path.clone())).unwrap();
            let restored = until(&client, |s| s.document.is_some() && !s.running);
            let reopen_elapsed = reopen.elapsed();
            assert_eq!(restored.coverage, (*positions, *positions));
            for (node, value) in &covered.values {
                assert!(restored.values[node].visits >= value.visits);
            }
            println!(
                "trial={trial} {} positions={positions} first={:.3}s full_chart={:.3}s cached_reopen={:.3}s",
                if index == 0 { "cold" } else { "warm" },
                first_elapsed.as_secs_f64(),
                chart_elapsed.as_secs_f64(),
                reopen_elapsed.as_secs_f64()
            );
        }
    }
}
