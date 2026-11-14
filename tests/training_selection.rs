use katastro::{
    Analysis, Point, Review, SuggestedMove,
    training::{Phase, Settings},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
#[path = "support/training.rs"]
mod support;
use support::*;

fn repeated(q: &Value, white: bool, key: &str) -> Value {
    let mut r = reply(q, white, false);
    let n = q["moves"]
        .as_array()
        .unwrap()
        .len()
        .saturating_sub(usize::from(white));
    let score = if n.is_multiple_of(2) { 8.0 } else { 2.0 };
    r["rootInfo"]["scoreLead"] = json!(if white { -score } else { score });
    if q["overrideSettings"]["analysisPVLen"].as_u64().unwrap_or(1) == 1 {
        r["moveInfos"] =
            json!([{"move":key,"visits":q["maxVisits"],"winrate":0.5,"scoreLead":8.0}]);
    }
    r
}
fn settings() -> Settings {
    Settings {
        moves: 1,
        include_completed: true,
        ..Default::default()
    }
}

// Both players keep missing the same move: retry is intentional, but Next must
// skip the whole opportunity in foreground discovery and a prepared catalog.
#[test]
fn repeated_misses_offer_one_problem_for_either_color_and_retry_stays_available() {
    for white in [false, true] {
        for prepared in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let source = fixture(temp.path(), white);
            let respond = |q: &Value| repeated(q, white, "D5");
            if prepared {
                let mut background = trainer(temp.path());
                background
                    .prepare(vec![source.clone()], "me".into(), false)
                    .unwrap();
                settle_with(&mut background, respond);
            }
            let mut t = trainer(temp.path());
            t.begin(vec![source], "me".into(), settings()).unwrap();
            settle_with(&mut t, respond);
            for attempt in 0..2 {
                assert_eq!(t.snapshot().board.unwrap().move_number, usize::from(white));
                t.play(Some(Point::new(0, 4))).unwrap();
                settle_with(&mut t, respond);
                assert_eq!(t.snapshot().phase, Phase::Failed);
                if attempt == 0 {
                    t.retry().unwrap();
                }
            }
            t.next_problem().unwrap();
            let requests = settle_with(&mut t, respond);
            assert_eq!(
                t.snapshot().phase,
                Phase::Idle,
                "do not offer another miss of D5"
            );
            if prepared {
                assert!(requests.is_empty(), "reuse the prepared grouping");
            }
        }
    }
}

#[test]
fn next_problem_keeps_distinct_key_moves_from_the_same_game() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let respond = |q: &Value| {
        repeated(
            q,
            false,
            if q["moves"].as_array().unwrap().len() < 4 {
                "D5"
            } else {
                "E5"
            },
        )
    };
    let mut t = trainer(temp.path());
    t.prepare(vec![source.clone()], "me".into(), false).unwrap();
    settle_with(&mut t, respond);
    t.begin(vec![source], "me".into(), settings()).unwrap();
    t.reveal().unwrap();
    settle_with(&mut t, respond);
    t.next_problem().unwrap();
    assert_eq!(
        t.snapshot().board.unwrap().move_number,
        4,
        "skip the second D5 miss, retain E5"
    );
}

// The point is played and captured; after the ko restriction expires, missing
// that intersection again is a separate opportunity, despite identical coordinates.
#[test]
fn a_played_intersection_starts_a_new_opportunity() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("ko.sgf");
    std::fs::write(&source, "(;SZ[9]PB[me]PW[opponent]RU[Japanese]AB[ag][bh]AW[bi]PL[B];B[cc];W[dd];B[ai];W[ah];B[ee];W[ff];B[gg];W[hh])").unwrap();
    let respond = |q: &Value| {
        let mut r = repeated(q, false, "A1");
        r["rootInfo"]["scoreLead"] =
            json!(if [1, 7].contains(&q["moves"].as_array().unwrap().len()) {
                2.0
            } else {
                8.0
            });
        r
    };
    let mut t = trainer(temp.path());
    t.prepare(vec![source.clone()], "me".into(), false).unwrap();
    settle_with(&mut t, respond);
    t.begin(vec![source], "me".into(), settings()).unwrap();
    t.reveal().unwrap();
    settle_with(&mut t, respond);
    t.next_problem().unwrap();
    assert_eq!(t.snapshot().board.unwrap().move_number, 6);
    assert!(
        t.snapshot()
            .document
            .unwrap()
            .append_move(Some(Point::new(0, 8)))
            .is_ok()
    );
}

#[test]
fn completed_opportunities_survive_restart_and_a_changed_representative() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let respond = |q: &Value| {
        let mut r = repeated(q, false, "D5");
        r["rootInfo"]["scoreLead"] = json!(match q["moves"].as_array().unwrap().len() {
            1 => 2.0,
            3 => 3.0,
            _ => 8.0,
        });
        r
    };
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), settings())
        .unwrap();
    settle_with(&mut t, respond);
    assert_eq!(t.snapshot().board.unwrap().move_number, 0);
    t.reveal().unwrap();
    settle_with(&mut t, respond);
    drop(t);
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    let doc = review.document().unwrap().clone();
    for &node in &doc.mainline {
        let score = match node {
            1 => 3.0,
            3 => 1.0,
            _ => 8.0,
        };
        review
            .store_analysis(
                &review.analysis_key(node, &profile()).unwrap(),
                &Analysis {
                    visits: 4096,
                    winrate: 0.5,
                    score_lead: score,
                    suggestions: vec![SuggestedMove {
                        point: Some(Point::new(3, 4)),
                        visits: 1000,
                        winrate: Some(0.5),
                        score_lead: Some(8.0),
                    }],
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
    assert!(background.next_request().unwrap().is_none());
    let mut t = trainer(temp.path());
    t.begin(
        vec![source.clone()],
        "me".into(),
        Settings {
            moves: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        t.snapshot().phase,
        Phase::Idle,
        "a different miss of the completed opportunity is still completed"
    );
    t.begin(vec![source], "me".into(), settings()).unwrap();
    assert_eq!(
        t.snapshot().board.unwrap().move_number,
        2,
        "explicit completed practice remains available"
    );
}

// Fixed seeds make the random-order check deterministic. Every eligible game
// must remain reachable, without replaying a group within the same session.
#[test]
fn seeded_game_order_varies_and_visits_each_ready_game_without_more_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let original = std::fs::read_to_string(fixture(temp.path(), false)).unwrap();
    let games: Vec<_> = (0..3)
        .map(|i| {
            let path = temp.path().join(format!("{i}.sgf"));
            std::fs::write(
                &path,
                original.replace("KM[6.5]", &format!("KM[{}]", 6.5 + i as f64)),
            )
            .unwrap();
            path
        })
        .collect();
    let mut background = trainer(temp.path());
    background
        .prepare(games.clone(), "me".into(), false)
        .unwrap();
    settle(&mut background, false, true);
    let mut first_games = BTreeSet::new();
    for seed in 0..8 {
        let mut orders = Vec::new();
        for _ in 0..2 {
            let mut t = trainer(temp.path()).with_seed(seed);
            t.begin(games.clone(), "me".into(), settings()).unwrap();
            let mut order = Vec::new();
            for _ in 0..games.len() {
                assert_eq!(t.snapshot().phase, Phase::YourTurn);
                assert!(
                    t.next_request().unwrap().is_none(),
                    "prepared exercise needs no discovery"
                );
                order.push(t.snapshot().document.unwrap().komi.to_string());
                t.reveal().unwrap();
                settle(&mut t, false, true);
                t.next_problem().unwrap();
            }
            assert_eq!(t.snapshot().phase, Phase::Idle);
            assert_eq!(order.iter().collect::<BTreeSet<_>>().len(), 3);
            orders.push(order);
        }
        assert_eq!(
            orders[0], orders[1],
            "same seed reproduces the problem order"
        );
        first_games.insert(orders[0][0].clone());
    }
    assert!(
        first_games.len() > 1,
        "do not always start with the newest game"
    );
}

// Legacy root-only caches need key-move metadata once; the cache-only review
// hook must defer that work, retain old progress, and let idle/foreground work fill it.
#[test]
fn legacy_progress_and_root_only_cache_are_upgraded_without_regrading_history() {
    for completed in [0, 2] {
        for metadata in ["root", "policy", "partial"] {
            let temp = tempfile::tempdir().unwrap();
            let source = fixture(temp.path(), false);
            let mut review =
                Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
            review.import(&source).unwrap();
            let doc = review.document().unwrap().clone();
            for &node in &doc.mainline {
                review
                    .store_analysis(
                        &review.analysis_key(node, &profile()).unwrap(),
                        &Analysis {
                            visits: 512,
                            winrate: 0.5,
                            score_lead: if node.is_multiple_of(2) { 8.0 } else { 2.0 },
                            suggestions: if metadata == "partial" && node != completed {
                                vec![SuggestedMove {
                                    point: Some(Point::new(3, 4)),
                                    visits: 512,
                                    winrate: Some(0.5),
                                    score_lead: Some(8.0),
                                }]
                            } else if metadata == "policy" {
                                vec![SuggestedMove {
                                    point: Some(Point::new(3, 4)),
                                    visits: 0,
                                    winrate: None,
                                    score_lead: None,
                                }]
                            } else {
                                vec![]
                            },
                            ownership: vec![],
                            ownership_visits: 0,
                        },
                    )
                    .unwrap();
            }
            let key = katastro::digest(
                &serde_json::to_vec(&(
                    "training-score-v2",
                    "me",
                    doc.source_identity().unwrap(),
                    doc.position(completed).unwrap(),
                    profile(),
                ))
                .unwrap(),
            );
            let saved = serde_json::to_string(&doc).unwrap();
            let db = rusqlite::Connection::open(temp.path().join("reviews")).unwrap();
            db.execute_batch("CREATE TABLE training_attempts (id INTEGER PRIMARY KEY, problem TEXT NOT NULL, outcome TEXT NOT NULL, report TEXT, document TEXT NOT NULL)").unwrap();
            db.execute("INSERT INTO training_attempts(problem,outcome,report,document) VALUES (?1,'passed','null',?2)", rusqlite::params![key,saved]).unwrap();
            let mut t = trainer(temp.path());
            t.prepare(vec![source.clone()], "me".into(), true).unwrap();
            assert!(
                t.next_request().unwrap().is_none(),
                "review indexing stays cache-only"
            );
            t.begin(
                vec![source],
                "me".into(),
                Settings {
                    moves: 1,
                    ..Default::default()
                },
            )
            .unwrap();
            let requests = settle_with(&mut t, |q| repeated(q, false, "D5"));
            assert!(!requests.is_empty(), "fill missing key-move metadata");
            assert!(
                requests.iter().all(|q| q["maxVisits"] == 512),
                "reuse cached screening and grade values"
            );
            assert_eq!(
                t.snapshot().phase,
                Phase::Idle,
                "old completion applies to every repeated miss"
            );
            let history: (String, String, String) = db
                .query_row(
                    "SELECT outcome,report,document FROM training_attempts",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(history, ("passed".into(), "null".into(), saved));
        }
    }
}

// Grouping must retain an earlier occurrence when its larger late-game miss
// does not have enough recorded moves for the selected attempt length.
#[test]
fn repeated_opportunity_keeps_an_occurrence_with_enough_recorded_moves() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let respond = |q: &Value| {
        let mut r = repeated(q, false, "D5");
        if q["moves"].as_array().unwrap().len() == 13 {
            r["rootInfo"]["scoreLead"] = json!(0.0);
        }
        r
    };
    let mut t = trainer(temp.path());
    t.prepare(vec![source.clone()], "me".into(), false).unwrap();
    settle_with(&mut t, respond);
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(t.snapshot().board.unwrap().move_number, 0);
    assert!(
        t.next_request().unwrap().is_none(),
        "warm the full-length occurrence"
    );
    t.begin(vec![source], "me".into(), settings()).unwrap();
    assert_eq!(t.snapshot().board.unwrap().move_number, 12);
}

// A deeper legacy root must not hide newly cached key-move metadata and
// force another search each time a completed review refreshes the catalog.
#[test]
fn deep_root_only_cache_reuses_new_metadata_after_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    for node in 0..15 {
        review
            .store_analysis(
                &review.analysis_key(node, &profile()).unwrap(),
                &Analysis {
                    visits: 4096,
                    winrate: 0.5,
                    score_lead: if node.is_multiple_of(2) { 8.0 } else { 2.0 },
                    suggestions: vec![],
                    ownership: vec![],
                    ownership_visits: 0,
                },
            )
            .unwrap();
    }
    let mut background = trainer(temp.path());
    background
        .prepare(vec![source.clone()], "me".into(), false)
        .unwrap();
    settle_with(&mut background, |q| repeated(q, false, "D5"));
    let mut refreshed = trainer(temp.path());
    refreshed
        .prepare(vec![source.clone()], "me".into(), true)
        .unwrap();
    assert!(refreshed.next_request().unwrap().is_none());
    let mut idle = trainer(temp.path());
    idle.prepare(vec![source.clone()], "me".into(), false)
        .unwrap();
    assert!(
        idle.next_request().unwrap().is_none(),
        "idle preparation reuses previously filled metadata too"
    );
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    assert_eq!(
        t.snapshot().phase,
        Phase::YourTurn,
        "refresh retains prepared grouping"
    );
    assert!(
        t.next_request().unwrap().is_none(),
        "reuse enriched metadata and grading window"
    );
    assert!(
        review
            .cached_analysis(&profile())
            .unwrap()
            .values()
            .all(|a| a.visits == 4096),
        "metadata searches never downgrade deep review values"
    );
}
