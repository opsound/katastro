use katastro::{
    Analysis, Document, Point, Review,
    training::{Phase, Settings},
};
#[path = "support/training.rs"]
mod support;
use support::*;

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
