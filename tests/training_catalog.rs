use katastro::{
    Analysis, Document, Point, Review,
    training::{Phase, Settings},
};
#[path = "support/training.rs"]
mod support;
use support::*;

#[test]
fn shallow_screening_resumes_into_statistics_and_training_from_the_same_scores() {
    use katastro::archive::{
        Game,
        categories::{Category, classify},
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("archived.sgf");
    let original = "(;SZ[9]KM[6.5]RE[W+R];B[aa];W[bb](;B[cc];W[dd])(;B[ff]))";
    std::fs::write(&source, original).unwrap();
    let game = Game {
        id: "shared".into(),
        date: "2026-01-02".into(),
        timestamp: None,
        white: "opponent".into(),
        black: "me".into(),
        result: "W+R".into(),
        path: source.clone(),
    };
    let mut review = Review::new(&root.join("reviews"), &root.join("cache")).unwrap();
    review.import(&source).unwrap();
    let opening = review.analysis_key(0, &profile()).unwrap();
    review
        .store_analysis(
            &opening,
            &Analysis {
                visits: 1024,
                score_lead: 0.,
                winrate: 0.5,
                suggestions: searched_moves(0, 1024),
                ownership: vec![],
                ownership_visits: 0,
            },
        )
        .unwrap();
    let mut interrupted = trainer(root);
    interrupted.screen_game(&source).unwrap();
    let first = interrupted
        .next_request()
        .unwrap()
        .expect("an unscored position must be screened");
    assert_eq!(first["analyzeTurns"][0], 1);
    assert_eq!(first["maxVisits"], 64);
    let mut result = reply(&first, false, false);
    result["rootInfo"]["scoreLead"] = (-8).into();
    interrupted.accept(&result, 0.5).unwrap();
    drop(interrupted);

    let mut background = trainer(root);
    background.screen_game(&source).unwrap();
    let remaining = settle_with(&mut background, |q| {
        let mut r = reply(q, false, false);
        r["rootInfo"]["scoreLead"] = 24.into();
        r
    });
    assert!(remaining.iter().all(|q| q["maxVisits"] == 64));
    assert_eq!(
        remaining
            .iter()
            .map(|q| q["analyzeTurns"][0].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert_eq!(background.snapshot().phase, Phase::Idle);
    assert_eq!(
        classify(&review, &game, "me", Some(&profile())).unwrap(),
        Category::Blunder
    );
    assert_eq!(review.cached_analysis(&profile()).unwrap()[&0].visits, 1024);
    // A surviving budget-specific entry can restore an evicted chart score.
    let evicted = review.analysis_key(2, &profile()).unwrap();
    let cache = rusqlite::Connection::open(root.join("cache")).unwrap();
    assert_eq!(
        cache
            .execute("DELETE FROM analysis WHERE key=?1", [&evicted])
            .unwrap(),
        1
    );
    background.screen_game(&source).unwrap();
    assert!(background.next_request().unwrap().is_none());
    assert!(
        review.cached_analysis(&profile()).unwrap().contains_key(&2),
        "Games needs the canonical score even when Training can reuse its budget-specific copy"
    );

    // Training on the same positions uses the screen even when the archived
    // copy obtains its player names from the index, and the review has SGF names.
    let named = root.join("named.sgf");
    std::fs::write(&named, original.replace("RE[", "PB[me]PW[opponent]RE[")).unwrap();
    background.prepare(vec![named], "me".into(), false).unwrap();
    let confirmation = background
        .next_request()
        .unwrap()
        .expect("a screened loss needs confirmation");
    assert_eq!(confirmation["maxVisits"], 512);
}

#[test]
fn shallow_screening_handles_a_missing_game_and_keeps_the_prefix_before_setup_edits() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("prefix.sgf");
    let mut t = trainer(temp.path());
    t.screen_game(&path).unwrap();
    assert!(t.next_request().unwrap().is_none());
    std::fs::write(&path, "(;SZ[9];B[aa];AB[cc];W[dd])").unwrap();
    t.screen_game(&path).unwrap();
    let scanned = settle(&mut t, false, false);
    assert_eq!(
        scanned
            .iter()
            .map(|q| q["analyzeTurns"][0].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1]
    );
}

// Benefit: ready problems open immediately after restart, even if a newer game
// has not been analyzed. Regression: starting a new screening pass first.
#[test]
fn prepared_problem_opens_before_an_unanalyzed_newer_game() {
    let temp = tempfile::tempdir().unwrap();
    let prepared = fixture(temp.path(), false);
    let mut background = trainer(temp.path());
    background
        .prepare(vec![prepared.clone()], "me".into(), false)
        .unwrap();
    settle(&mut background, false, true);
    drop(background);
    let newer = temp.path().join("newer.sgf");
    std::fs::write(
        &newer,
        std::fs::read_to_string(prepared.clone())
            .unwrap()
            .replace("KM[6.5]", "KM[7.5]"),
    )
    .unwrap();
    let mut t = trainer(temp.path());
    t.begin(vec![newer, prepared], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(
        t.snapshot().phase,
        Phase::YourTurn,
        "use a prepared candidate without screening the newer game"
    );
    assert_eq!(t.snapshot().document.unwrap().komi, 6.5);
    t.play(Some(Point::new(3, 4))).unwrap();
    assert_eq!(t.next_request().unwrap().unwrap()["includePolicy"], true);
}

// Benefit: a completed review supplies discovery without duplicate engine work,
// and deeper completed passes can remove a formerly qualifying mistake.
#[test]
fn preparation_reuses_deep_review_analysis_and_refreshes_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let doc = Document::parse(&std::fs::read(&source).unwrap()).unwrap();
    // Keep older exact-budget entries alongside the newer normal review results.
    let mut older = trainer(temp.path());
    older
        .prepare(vec![source.clone()], "me".into(), false)
        .unwrap();
    settle(&mut older, false, true);
    drop(older);
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    for (round, qualifies) in [(1024, true), (4096, false)] {
        for &node in &doc.mainline {
            let good = node == 0 || !qualifies;
            review
                .store_analysis(
                    &review.analysis_key(node, &profile()).unwrap(),
                    &Analysis {
                        visits: round,
                        winrate: if good { 0.8 } else { 0.4 },
                        score_lead: if good { 8.0 } else { -2.0 },
                        suggestions: support::searched_moves(node, 64),
                        ownership: vec![],
                        ownership_visits: 0,
                    },
                )
                .unwrap();
        }
        let mut background = trainer(temp.path());
        background
            .prepare(vec![source.clone()], "me".into(), true)
            .unwrap();
        assert!(
            background.next_request().unwrap().is_none(),
            "reuse completed review evaluations for discovery"
        );
        let mut t = trainer(temp.path());
        t.begin(vec![source.clone()], "me".into(), Settings::default())
            .unwrap();
        assert_eq!(
            t.snapshot().phase,
            if qualifies {
                Phase::YourTurn
            } else {
                Phase::Idle
            }
        );
        if qualifies {
            t.play(Some(Point::new(3, 4))).unwrap();
            assert_eq!(t.next_request().unwrap().unwrap()["includePolicy"], true);
        }
    }
    review.clear_analysis().unwrap();
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    assert!(
        t.next_request().unwrap().is_some(),
        "clearing analysis also invalidates negative discovery results"
    );
}

// Benefit: resuming preparation on the same engine cannot mistake an old reply
// for a new game's evaluation and manufacture a training problem.
#[test]
fn old_preparation_replies_cannot_create_a_candidate_in_a_new_preparer() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let other = temp.path().join("other.sgf");
    std::fs::write(
        &other,
        std::fs::read_to_string(&source)
            .unwrap()
            .replace("KM[6.5]", "KM[7.5]"),
    )
    .unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    for path in [&source, &other] {
        review.import(path).unwrap();
        for node in 0..15 {
            review
                .store_analysis(
                    &review.analysis_key(node, &profile()).unwrap(),
                    &Analysis {
                        visits: 64,
                        winrate: if node == 0 { 0.8 } else { 0.4 },
                        score_lead: if node == 0 { 8.0 } else { -2.0 },
                        suggestions: support::searched_moves(node, 64),
                        ownership: vec![],
                        ownership_visits: 0,
                    },
                )
                .unwrap();
        }
    }
    let mut old = trainer(temp.path());
    old.prepare(vec![source], "me".into(), false).unwrap();
    let stale = old.next_request().unwrap().unwrap();
    let mut current = trainer(temp.path());
    current
        .prepare(vec![other.clone()], "me".into(), false)
        .unwrap();
    let root = current.next_request().unwrap().unwrap();
    current.accept(&reply(&stale, false, true), 0.5).unwrap();
    let mut neutral = reply(&root, false, false);
    neutral["rootInfo"]["winrate"] = serde_json::json!(0.4);
    neutral["rootInfo"]["scoreLead"] = serde_json::json!(-2.0);
    current.accept(&neutral, 0.5).unwrap();
    settle(&mut current, false, false);
    let mut t = trainer(temp.path());
    t.begin(vec![other], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(
        t.snapshot().phase,
        Phase::Idle,
        "late confirmation must not fabricate a lost lead"
    );
}

#[test]
fn preparation_collects_all_problems_and_next_respects_the_horizon_without_completing_failures() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let respond = |q: &serde_json::Value| {
        let mut r = reply(q, false, false);
        let leading = q["moves"].as_array().unwrap().len().is_multiple_of(2);
        r["rootInfo"]["winrate"] = serde_json::json!(if leading { 0.8 } else { 0.4 });
        r["rootInfo"]["scoreLead"] = serde_json::json!(if leading { 8.0 } else { -2.0 });
        r
    };
    let mut background = trainer(temp.path());
    background
        .prepare(vec![source.clone()], "me".into(), false)
        .unwrap();
    settle_with(&mut background, respond);
    assert_eq!(background.snapshot().phase, Phase::Idle);
    drop(background);
    let mut t = trainer(temp.path());
    t.begin(
        vec![source.clone()],
        "me".into(),
        Settings {
            include_completed: true,
            ..Default::default()
        },
    )
    .unwrap();
    for move_number in [0, 2, 4] {
        assert_eq!(t.snapshot().board.unwrap().move_number, move_number);
        assert!(
            t.next_request().unwrap().is_none(),
            "every prepared problem should reuse confirmation"
        );
        t.play(None).unwrap();
        settle_with(&mut t, respond);
        assert_eq!(t.snapshot().phase, Phase::Failed);
        assert_eq!(
            t.snapshot().report.unwrap().comparison.attempt.len(),
            3,
            "early finish truncates the comparison too"
        );
        t.next_problem().unwrap();
    }
    assert_eq!(
        t.snapshot().phase,
        Phase::Idle,
        "do not offer positions too near the end for five moves"
    );
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(
        t.snapshot().phase,
        Phase::YourTurn,
        "skipped failures remain eligible in a later session"
    );
    assert_eq!(t.snapshot().board.unwrap().move_number, 0);
}

#[test]
fn a_game_with_no_candidates_stays_indexed_after_screening_values_are_evicted() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut background = trainer(temp.path());
    background
        .prepare(vec![source.clone()], "me".into(), false)
        .unwrap();
    settle_with(&mut background, |q| {
        let mut r = reply(q, false, false);
        r["rootInfo"]["winrate"] = serde_json::json!(0.5);
        r["rootInfo"]["scoreLead"] = serde_json::json!(0.0);
        r
    });
    drop(background);
    // Evict individual screening entries, retaining the derived catalog. The
    // explicit clear-analysis command instead invalidates both, tested above.
    rusqlite::Connection::open(temp.path().join("cache"))
        .unwrap()
        .execute("DELETE FROM analysis WHERE visits=64", [])
        .unwrap();
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(t.snapshot().phase, Phase::Idle);
    assert!(
        t.next_request().unwrap().is_none(),
        "a completed negative scan must not be repeated"
    );
}

#[test]
fn candidate_identity_includes_player_color_position_and_engine_profile() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let original = std::fs::read_to_string(&source).unwrap();
    let mut background = trainer(temp.path());
    background
        .prepare(vec![source.clone()], "me".into(), false)
        .unwrap();
    settle(&mut background, false, true);
    std::fs::write(
        &source,
        original.replace("PB[me]PW[opponent]", "PB[opponent]PW[me]"),
    )
    .unwrap();
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    assert_eq!(
        t.snapshot().phase,
        Phase::Idle,
        "the original candidate was Black's mistake, not White's"
    );
    std::fs::write(&source, original.replace("KM[6.5]", "KM[7.5]")).unwrap();
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(t.snapshot().phase, Phase::Searching);
    assert_eq!(t.next_request().unwrap().unwrap()["maxVisits"], 64);
    std::fs::write(&source, original).unwrap();
    let mut changed = profile();
    changed.model_sha256 = "another-model".into();
    let mut t = katastro::training::Trainer::new(
        &temp.path().join("reviews"),
        &temp.path().join("cache"),
        &temp.path().join("Training"),
        changed,
    )
    .unwrap();
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(t.snapshot().phase, Phase::Searching);
    assert_eq!(t.next_request().unwrap().unwrap()["maxVisits"], 64);
}
