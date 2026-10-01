use katastro::{Analysis, EngineProfile, Point, Review};
use std::fs;
use tempfile::TempDir;
fn profile() -> EngineProfile {
    EngineProfile {
        model_sha256: "model-a".into(),
        engine_sha256: "engine-a".into(),
        settings_digest: "black-perspective-v1".into(),
    }
}
fn sample(visits: u64) -> Analysis {
    Analysis {
        ownership: vec![],
        ownership_visits: 0,
        visits,
        winrate: 0.0,
        score_lead: 0.0,
        suggestions: vec![],
    }
}
#[test]
fn reopen_restores_partial_analysis_and_deeper_results_without_an_engine() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    fs::write(&source, "(;SZ[9];B[cc];W[gg])").unwrap();
    let mut app = Review::new(
        &temp.path().join("review.sqlite"),
        &temp.path().join("cache.sqlite"),
    )
    .unwrap();
    app.import(&source).unwrap();
    let line = app.document().unwrap().mainline.clone();
    let key = app.analysis_key(line[1], &profile()).unwrap();
    app.store_analysis(&key, &sample(64)).unwrap();
    app.store_analysis(&key, &sample(1)).unwrap();
    assert!(
        app.store_analysis(
            &key,
            &Analysis {
                ownership: vec![],
                ownership_visits: 0,
                visits: 5,
                winrate: f64::NAN,
                score_lead: 3.0,
                suggestions: vec![],
            }
        )
        .is_err()
    );
    drop(app);
    let mut app = Review::new(
        &temp.path().join("review.sqlite"),
        &temp.path().join("cache.sqlite"),
    )
    .unwrap();
    let renamed = temp.path().join("renamed.sgf");
    fs::rename(&source, &renamed).unwrap();
    app.import(&renamed).unwrap();
    let results = app.cached_analysis(&profile()).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[&line[1]], sample(64));
    assert!(!results.contains_key(&line[0]));
    let mut changed = profile();
    changed.model_sha256 = "other-model".into();
    assert!(app.cached_analysis(&changed).unwrap().is_empty());
    changed = profile();
    changed.engine_sha256 = "other-engine".into();
    assert!(app.cached_analysis(&changed).unwrap().is_empty());
    changed = profile();
    changed.settings_digest = "other-settings".into();
    assert!(app.cached_analysis(&changed).unwrap().is_empty());
}
#[test]
fn cache_identity_distinguishes_history_rules_komi_setup_side_and_size() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    let mut app = Review::new(
        &temp.path().join("review.sqlite"),
        &temp.path().join("cache.sqlite"),
    )
    .unwrap();
    let cases = [
        "(;SZ[9]KM[7.5]RU[Chinese];B[cc];W[gg])",
        "(;SZ[9]KM[6.5]RU[Chinese];B[cc];W[gg])",
        "(;SZ[9]KM[7.5]RU[Japanese];B[cc];W[gg])",
        "(;SZ[9]KM[7.5]RU[Chinese]AB[aa];B[cc];W[gg])",
        "(;SZ[9]KM[7.5]RU[Chinese]PL[W];W[gg];B[cc])",
        "(;SZ[13]KM[7.5]RU[Chinese];B[cc];W[gg])",
        "(;SZ[9]KM[7.5]RU[Chinese];B[cc];W[];B[];W[gg])",
    ];
    let mut keys = std::collections::HashSet::new();
    for sgf in cases {
        fs::write(&source, sgf).unwrap();
        app.import(&source).unwrap();
        let last = *app.document().unwrap().mainline.last().unwrap();
        assert!(keys.insert(app.analysis_key(last, &profile()).unwrap()));
    }
}
#[test]
fn clearing_analysis_does_not_erase_variations_and_comments_do_not_change_analysis_identity() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    fs::write(&source, "(;SZ[9];B[cc])").unwrap();
    let mut app = Review::new(
        &temp.path().join("review.sqlite"),
        &temp.path().join("cache.sqlite"),
    )
    .unwrap();
    app.import(&source).unwrap();
    let key = app.analysis_key(1, &profile()).unwrap();
    app.store_analysis(&key, &sample(64)).unwrap();
    assert_eq!(app.cached_analysis(&profile()).unwrap().len(), 1);
    app.select(1).unwrap();
    app.play(Some(Point::new(3, 3))).unwrap();
    app.clear_analysis().unwrap();
    drop(app);
    let mut app = Review::new(
        &temp.path().join("review.sqlite"),
        &temp.path().join("cache.sqlite"),
    )
    .unwrap();
    app.import(&source).unwrap();
    assert_eq!(app.document().unwrap().nodes.len(), 3);
    assert!(app.cached_analysis(&profile()).unwrap().is_empty());
    fs::write(&source, "(;SZ[9]C[a new comment];B[cc]C[another])").unwrap();
    app.import(&source).unwrap();
    assert_eq!(app.analysis_key(1, &profile()).unwrap(), key);
    assert_eq!(
        app.document().unwrap().nodes.len(),
        3,
        "metadata edits must not discard saved variations"
    );
}

#[test]
fn cheap_results_are_transient_and_only_deepest_eligible_results_are_saved() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    fs::write(&source, "(;SZ[9];B[cc];W[gg])").unwrap();
    let reviews = temp.path().join("reviews");
    let cache = temp.path().join("cache");
    let mut app = Review::new(&reviews, &cache).unwrap();
    app.import(&source).unwrap();
    for visits in [1, 8] {
        let key = app.analysis_key(0, &profile()).unwrap();
        app.store_analysis(&key, &sample(visits)).unwrap();
        assert!(
            app.cached_analysis(&profile()).unwrap().is_empty(),
            "cheap estimates leaked to disk"
        );
    }
    let key = app.analysis_key(1, &profile()).unwrap();
    for visits in [64, 1024, 256, 64] {
        app.store_analysis(&key, &sample(visits)).unwrap();
    }
    app.select(1).unwrap();
    let branch = app.play(Some(Point::new(3, 3))).unwrap();
    drop(app);
    // Simulate an older installation containing an obsolete one-visit row.
    let conn = rusqlite::Connection::open(&cache).unwrap();
    conn.execute(
        "INSERT INTO analysis(key,visits,result) VALUES ('legacy-cheap',1,?1)",
        [serde_json::to_string(&sample(1)).unwrap()],
    )
    .unwrap();
    // Older deep rows had no candidate field; they still supply a chart estimate.
    conn.execute(
        "UPDATE analysis SET result=?1 WHERE visits=1024",
        [r#"{"visits":1024,"winrate":0.0,"score_lead":0.0}"#],
    )
    .unwrap();
    drop(conn);
    let mut reopened = Review::new(&reviews, &cache).unwrap();
    reopened.import(&source).unwrap();
    assert_eq!(reopened.document().unwrap().selected, branch);
    assert_eq!(
        reopened.cached_analysis(&profile()).unwrap()[&1].visits,
        1024
    );
    let conn = rusqlite::Connection::open(&cache).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM analysis", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn chart_and_group_maps_keep_their_deepest_completed_depths_independently() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    fs::write(&source, b"(;SZ[9];B[bb])").unwrap();
    let mut app = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    app.import(&source).unwrap();
    let key = app.analysis_key(1, &profile()).unwrap();
    app.store_analysis(&key, &sample(16384)).unwrap();
    let mut transient_on_deep_chart = sample(16384);
    transient_on_deep_chart.ownership = vec![0.7; 81];
    transient_on_deep_chart.ownership_visits = 1;
    app.store_analysis(&key, &transient_on_deep_chart).unwrap();
    assert!(
        app.cached_analysis(&profile()).unwrap()[&1]
            .ownership
            .is_empty(),
        "a cheap preview carried with a deep chart must stay off disk"
    );
    let mut map = sample(64);
    map.ownership = vec![0.9; 81];
    map.ownership_visits = 64;
    app.store_analysis(&key, &map).unwrap();
    let cached = app.cached_analysis(&profile()).unwrap();
    assert_eq!(cached[&1].visits, 16384);
    assert_eq!(
        cached[&1].ownership_depth(),
        64,
        "a completed map must backfill missing legacy ownership without replacing the chart"
    );
    map.visits = 256;
    map.ownership_visits = 256;
    map.ownership.fill(-0.9);
    app.store_analysis(&key, &map).unwrap();
    map.visits = 64;
    map.ownership_visits = 64;
    map.ownership.fill(0.0);
    app.store_analysis(&key, &map).unwrap();
    app.store_analysis(&key, &sample(65536)).unwrap();
    let mut quick_on_deep_chart = sample(65536);
    quick_on_deep_chart.ownership = vec![0.5; 81];
    quick_on_deep_chart.ownership_visits = 1;
    app.store_analysis(&key, &quick_on_deep_chart).unwrap();
    drop(app);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&source).unwrap();
    let stored = reopened.cached_analysis(&profile()).unwrap();
    assert_eq!(stored[&1].visits, 65536);
    assert_eq!(stored[&1].ownership_depth(), 256);
    assert_eq!(
        stored[&1].ownership,
        vec![-0.9; 81],
        "late cheap maps and deep replies without ownership must preserve the best complete map"
    );
}
