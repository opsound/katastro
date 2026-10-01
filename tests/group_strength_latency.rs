use katastro::{
    Analysis, Review,
    engine::{Engine, EngineConfig},
    worker::{Client, Command, Snapshot},
};
use rusqlite::{Connection, OpenFlags};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn until(client: &Client, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let s = client
            .recv(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(s.error.is_none(), "{:?}", s.error);
        if predicate(&s) {
            return s;
        }
        assert!(Instant::now() < deadline, "{}", s.status);
    }
}
#[test]
#[ignore = "Requires real Metal KataGo, private SGFs, and read-only existing legacy analysis cache"]
fn live_selected_group_latency_with_real_legacy_chart_cache() {
    let config = EngineConfig {
        executable: std::env::var_os("KATASTRO_TEST_ENGINE")
            .expect("engine")
            .into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL")
            .expect("model")
            .into(),
    };
    let corpus = PathBuf::from(std::env::var_os("KATASTRO_SGF_DIR").expect("SGFs"));
    let old_cache = Connection::open_with_flags(
        PathBuf::from(std::env::var_os("KATASTRO_LEGACY_CACHE").expect("read-only legacy cache")),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let rows: Vec<(String, Analysis)> = old_cache
        .prepare("SELECT key,result FROM analysis")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(|r| {
            let (k, v) = r.unwrap();
            (k, serde_json::from_str(&v).unwrap())
        })
        .collect();
    drop(old_cache);
    let profile = Engine::profile(&config).unwrap();
    let seed = tempfile::TempDir::new().unwrap();
    let mut review = Review::new(&seed.path().join("reviews"), &seed.path().join("cache")).unwrap();
    for (key, value) in &rows {
        review.store_analysis(key, value).unwrap();
    }
    let mut games = Vec::new();
    for entry in std::fs::read_dir(&corpus).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|e| e == "sgf") {
            continue;
        }
        review.import(&path).unwrap();
        let doc = review.document().unwrap();
        if doc.size != 19 || doc.mainline.len() < 150 {
            continue;
        }
        let cached = review.cached_analysis(&profile).unwrap();
        let middle = doc.mainline.len() / 2;
        let selected = doc
            .mainline
            .iter()
            .skip(20)
            .copied()
            .filter(|node| {
                cached
                    .get(node)
                    .is_some_and(|a| a.visits >= 4096 && a.ownership.is_empty())
            })
            .min_by_key(|node| node.abs_diff(middle));
        if let Some(node) = selected {
            games.push((doc.mainline.len(), path, node, cached[&node].clone()));
        }
    }
    games.sort_by_key(|g| g.0);
    assert!(
        games.len() >= 2,
        "need two analyzed 19x19 games with missing legacy ownership"
    );
    let samples = [&games[games.len() / 2], &games[games.len() - 1]];
    for trial in 1..=3 {
        let temp = tempfile::TempDir::new().unwrap();
        let reviews = temp.path().join("reviews");
        let cache = temp.path().join("cache");
        let mut review = Review::new(&reviews, &cache).unwrap();
        for (key, value) in &rows {
            review.store_analysis(key, value).unwrap();
        }
        for (_, path, node, _) in samples {
            review.import(path).unwrap();
            review.select(*node).unwrap();
        }
        review.save_profile(&profile).unwrap();
        let client = Client::spawn(reviews, cache, config.clone());
        let warm = temp.path().join("warm.sgf");
        std::fs::write(&warm, b"(;SZ[19];B[dd];W[pp])").unwrap();
        client.send(Command::OpenAndAnalyze(warm)).unwrap();
        until(&client, |s| s.coverage == (3, 3));
        client.send(Command::Pause).unwrap();
        until(&client, |s| !s.running && s.document.is_some());
        for (index, (positions, path, node, legacy)) in samples.iter().enumerate() {
            let start = Instant::now();
            client.send(Command::OpenAndAnalyze(path.clone())).unwrap();
            let visible = until(&client, |s| {
                s.source.as_ref() == Some(path)
                    && s.values.get(node).is_some_and(|a| !a.ownership.is_empty())
            });
            let elapsed = start.elapsed();
            let value = &visible.values[node];
            assert!(
                value.visits >= legacy.visits,
                "quick group estimates must not downgrade the chart"
            );
            let json = serde_json::to_value(value).unwrap();
            let ownership_visits = json["ownership_visits"]
                .as_u64()
                .filter(|v| *v > 0)
                .unwrap_or(value.visits);
            println!(
                "trial={trial} sample={index} positions={positions} selected={node} legacy_visits={} group_visits={ownership_visits} visible={:.3}s",
                legacy.visits,
                elapsed.as_secs_f64()
            );
            client.send(Command::Pause).unwrap();
            until(&client, |s| !s.running && s.document.is_some());
        }
    }
}
