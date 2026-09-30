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
        visits,
        winrate: 0.0,
        score_lead: 0.0,
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
                visits: 5,
                winrate: f64::NAN,
                score_lead: 3.0
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
    app.store_analysis(&key, &sample(1)).unwrap();
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
