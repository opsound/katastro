//! Opt-in comparison on the same private corpus, engine/model, and warm process.
use katastro::{
    Document, Point, Review, digest,
    engine::EngineConfig,
    worker::{Client, Command, Snapshot},
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn until(client: &Client, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        let state = client
            .recv(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(state.error.is_none(), "{:?}", state.error);
        if done(&state) {
            return state;
        }
        assert!(Instant::now() < deadline, "analysis did not finish");
    }
}
fn pause(client: &Client) {
    client.send(Command::Pause).unwrap();
    until(client, |s| !s.running);
}
fn samples(root: &Path) -> Vec<(String, PathBuf, Document)> {
    let mut games: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|e| {
            let path = e.ok()?.path();
            if path.extension()?.to_str()? != "sgf" {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            let doc = Document::parse(&bytes).ok()?;
            (doc.size == 19
                && (150..=350).contains(&doc.mainline.len())
                && doc.position(*doc.mainline.last()?).is_ok())
            .then(|| (digest(&bytes), path, doc))
        })
        .collect();
    games.sort_by(|a, b| (a.2.mainline.len(), &a.0).cmp(&(b.2.mainline.len(), &b.0)));
    assert!(games.len() >= 2, "need two representative 19x19 games");
    let second = games.swap_remove(games.len() * 3 / 4);
    vec![games.swap_remove(games.len() / 4), second]
}
#[test]
#[ignore = "Requires real KataGo, a model and private SGFs; records comparable throughput and interaction latency"]
fn real_worker_throughput_and_interaction_latency() {
    let config = EngineConfig {
        analysis: Default::default(),
        executable: std::env::var_os("KATASTRO_TEST_ENGINE")
            .expect("engine")
            .into(),
        model: std::env::var_os("KATASTRO_TEST_MODEL")
            .expect("model")
            .into(),
    };
    let games = samples(&PathBuf::from(
        std::env::var_os("KATASTRO_SGF_DIR").expect("SGFs"),
    ));
    let profile = katastro::engine::Engine::profile(&config).unwrap();
    let mut measurements = Vec::new();
    let trial_filter = std::env::var("KATASTRO_UTIL_TRIAL")
        .ok()
        .map(|n| n.parse::<usize>().unwrap());
    let sample_filter = std::env::var("KATASTRO_UTIL_SAMPLE")
        .ok()
        .map(|n| n.parse::<usize>().unwrap());
    for trial in (0..3).filter(|n| trial_filter.is_none_or(|t| *n == t)) {
        for (sample, (hash, _, original)) in games
            .iter()
            .enumerate()
            .filter(|(n, _)| sample_filter.is_none_or(|s| *n == s))
        {
            let temp = tempfile::tempdir().unwrap();
            // Fixed real-game prefixes bound runtime without publishing private SGFs.
            let mut doc = original.clone();
            doc.nodes = original
                .mainline
                .iter()
                .take(97)
                .map(|id| original.nodes[*id].clone())
                .collect();
            let count = doc.nodes.len();
            for (id, node) in doc.nodes.iter_mut().enumerate() {
                node.parent = id.checked_sub(1);
                node.children = (id + 1 < count).then_some(id + 1).into_iter().collect();
            }
            doc.mainline = (0..count).collect();
            doc.selected = 0;
            let path = temp.path().join("sample.sgf");
            std::fs::write(&path, doc.to_sgf()).unwrap();
            let path = path.canonicalize().unwrap();
            let path = &path;
            println!("trial={trial} sample={sample} phase=mainline");
            let db = temp.path().join("reviews");
            let cache = temp.path().join("cache");
            let client = Client::spawn(db.clone(), cache.clone(), config.clone());
            // Warm inference without analyzing any position from the measured game.
            let warm = temp.path().join("warm.sgf");
            std::fs::write(&warm, b"(;SZ[19]KM[6.5];B[jj];W[dd])").unwrap();
            client.send(Command::OpenAndAnalyze(warm)).unwrap();
            until(&client, |s| s.coverage == (3, 3));
            pause(&client);
            let started = Instant::now();
            client.send(Command::OpenAndAnalyze(path.clone())).unwrap();
            until(&client, |s| {
                s.source.as_deref() == Some(path)
                    && s.coverage == (doc.mainline.len(), doc.mainline.len())
            });
            let chart = started.elapsed().as_secs_f64();
            until(&client, |s| {
                doc.mainline
                    .iter()
                    .all(|id| s.values.get(id).is_some_and(|a| a.visits >= 64))
            });
            let mainline = started.elapsed().as_secs_f64();
            pause(&client);
            drop(client);
            // Add 24 independent legal branches to real middle-game positions.
            let mut review = Review::new(&db, &cache).unwrap();
            review.import(path).unwrap();
            let saved = review.cached_analysis(&profile).unwrap();
            assert!(
                doc.mainline
                    .iter()
                    .all(|id| saved.get(id).is_some_and(|a| a.visits >= 64))
            );
            let mut branches = Vec::new();
            for offset in 0..24 {
                let parent = doc.mainline[20 + offset * (doc.mainline.len() - 40) / 24];
                review.select(parent).unwrap();
                let before = review.document().unwrap().nodes.len();
                for y in 0..19 {
                    for x in 0..19 {
                        review.select(parent).unwrap();
                        if let Ok(node) = review.play(Some(Point::new(x, y)))
                            && node >= before
                        {
                            branches.push(node);
                            break;
                        }
                    }
                    if branches.len() > offset {
                        break;
                    }
                }
            }
            assert_eq!(branches.len(), 24);
            let warm_client = Client::spawn(db.clone(), cache.clone(), config.clone());
            warm_client
                .send(Command::OpenAndAnalyze(temp.path().join("warm.sgf")))
                .unwrap();
            until(&warm_client, |s| s.coverage == (3, 3));
            pause(&warm_client);
            println!("trial={trial} sample={sample} phase=branches");
            let started = Instant::now();
            warm_client
                .send(Command::OpenAndAnalyze(path.clone()))
                .unwrap();
            until(&warm_client, |s| {
                s.source.as_deref() == Some(path) && s.running
            });
            let mut latency = Vec::new();
            let mut first_estimate = Vec::new();
            for node in branches.iter().step_by(3).take(8) {
                let selected = Instant::now();
                warm_client.send(Command::Select(*node)).unwrap();
                let visible = until(&warm_client, |s| {
                    s.document.as_ref().is_some_and(|d| d.selected == *node)
                        && s.values.contains_key(node)
                });
                first_estimate.push(selected.elapsed().as_secs_f64());
                if visible.values[node].visits < 64 {
                    until(&warm_client, |s| {
                        s.document.as_ref().is_some_and(|d| d.selected == *node)
                            && s.values.get(node).is_some_and(|a| a.visits >= 64)
                    });
                }
                latency.push(selected.elapsed().as_secs_f64());
            }
            until(&warm_client, |s| {
                branches
                    .iter()
                    .all(|id| s.values.get(id).is_some_and(|a| a.visits >= 64))
            });
            let branches_time = started.elapsed().as_secs_f64();
            pause(&warm_client);
            let saved = review.cached_analysis(&profile).unwrap();
            assert!(
                branches
                    .iter()
                    .all(|id| saved.get(id).is_some_and(|a| a.visits >= 64))
            );
            let row = json!({"trial":trial,"sample":sample,"source_sha256":hash,"positions":doc.mainline.len(),
                "chart_seconds":chart,"mainline_64_seconds":mainline,
                "branches_64_seconds":branches_time,"selection_seconds":latency,"first_estimate_seconds":first_estimate});
            println!("{row}");
            measurements.push(row);
        }
    }
    let result = json!({"profile":profile,"measurements":measurements});
    let output = std::env::var_os("KATASTRO_UTIL_RECORD").expect("KATASTRO_UTIL_RECORD");
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
