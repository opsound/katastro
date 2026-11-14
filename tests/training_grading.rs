use katastro::{
    Document, Point,
    training::{Phase, Settings, Trainer},
};
use serde_json::Value;
#[path = "support/training.rs"]
mod support;
use support::*;

fn discover(t: &mut Trainer) {
    while t.snapshot().phase == Phase::Searching {
        let q = t.next_request().unwrap().expect("discovery stalled");
        t.accept(&reply(&q, false, true), 0.5).unwrap();
    }
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
}
fn drain(t: &mut Trainer, held: &mut Vec<Value>, hold: impl Fn(&Value) -> bool) {
    while let Some(q) = t.next_request().unwrap() {
        if hold(&q) {
            held.push(q);
        } else {
            t.accept(&reply(&q, false, true), 0.5).unwrap();
        }
    }
}
fn endpoint(q: &Value) -> bool {
    q["includePolicy"] != true && q["moves"].as_array().unwrap().len() == 10
}

// Benefit: both idle preparation and a newly opened exercise use time before
// play to cache the recorded window. Reopening must actually reuse that work.
#[test]
fn recorded_window_is_prepared_before_play_and_reused_after_restart() {
    for background in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path(), false);
        let doc = Document::parse(&std::fs::read(&source).unwrap()).unwrap();
        let mut t = trainer(temp.path());
        if background {
            t.prepare(vec![source.clone()], "me".into(), false).unwrap();
        } else {
            t.begin(vec![source.clone()], "me".into(), Settings::default())
                .unwrap();
        }
        let prepared = settle(&mut t, false, true);
        assert!(
            prepared
                .iter()
                .any(|q| q["maxVisits"] == 512 && q["moves"].as_array().unwrap().len() == 10),
            "the full recorded comparison must be evaluated before the first move"
        );
        drop(t);
        let mut t = trainer(temp.path());
        t.begin(vec![source], "me".into(), Settings::default())
            .unwrap();
        assert_eq!(t.snapshot().phase, Phase::YourTurn);
        assert!(t.next_request().unwrap().is_none());
        for i in 0..5 {
            t.play(Some(Point::new(i, 4))).unwrap();
            for q in settle(&mut t, false, true) {
                let played = q["moves"].as_array().unwrap().len();
                assert_ne!(
                    q["moves"],
                    serde_json::to_value(doc.position(doc.mainline[played]).unwrap()).unwrap()["moves"],
                    "recorded positions should not be searched again during the attempt"
                );
            }
        }
        let report = t.snapshot().report.unwrap();
        assert!(report.passed);
        assert_eq!(report.comparison.recorded.len(), 11);
        assert_eq!(report.comparison.attempt.len(), 11);
        assert!((report.recorded_loss.winrate - 0.4).abs() < 1e-9);
    }
}

// Benefit: the last AI reply can begin its graph evaluation immediately while
// an earlier move is still being scored; a late result must not grade too soon.
#[test]
fn final_reply_queues_its_evaluation_without_waiting_for_other_results() {
    let temp = tempfile::tempdir().unwrap();
    let mut t = trainer(temp.path());
    t.begin(
        vec![fixture(temp.path(), false)],
        "me".into(),
        Settings::default(),
    )
    .unwrap();
    settle(&mut t, false, true);
    for i in 0..4 {
        t.play(Some(Point::new(i, 4))).unwrap();
        settle(&mut t, false, true);
    }
    t.play(Some(Point::new(4, 4))).unwrap();
    let human = t.next_request().unwrap().unwrap();
    assert_eq!(human["includePolicy"], true);
    let own = t.next_request().unwrap().unwrap();
    t.accept(&reply(&human, false, true), 0.5).unwrap();
    let end = t
        .next_request()
        .unwrap()
        .expect("final endpoint waited for an unrelated search");
    assert!(endpoint(&end));
    t.accept(&reply(&end, false, true), 0.5).unwrap();
    assert!(
        t.snapshot().report.is_none(),
        "the final own move still needs grading"
    );
    t.accept(&reply(&own, false, true), 0.5).unwrap();
    settle(&mut t, false, true);
    assert!(t.snapshot().report.unwrap().passed);
}

// Benefit: pass/fail depends only on own-move losses. Finishing the comparison
// later must update the same saved attempt, without changing the grade.
#[test]
fn grading_finishes_before_chart_endpoints_then_updates_one_saved_report() {
    let temp = tempfile::tempdir().unwrap();
    let mut t = trainer(temp.path());
    t.begin(
        vec![fixture(temp.path(), false)],
        "me".into(),
        Settings::default(),
    )
    .unwrap();
    discover(&mut t);
    let mut held = vec![];
    drain(&mut t, &mut held, endpoint);
    for i in 0..5 {
        t.play(Some(Point::new(i, 4))).unwrap();
        drain(&mut t, &mut held, endpoint);
    }
    assert_eq!(
        t.snapshot().phase,
        Phase::Passed,
        "chart-only work must not hold up pass/fail"
    );
    let report = t.snapshot().report.unwrap();
    assert_eq!(report.comparison.recorded.len(), 10);
    assert_eq!(report.comparison.attempt.len(), 10);
    assert_eq!(held.len(), 2);
    let db = rusqlite::Connection::open(temp.path().join("reviews")).unwrap();
    let saved = || {
        db.query_row("SELECT count(*), report FROM training_attempts", [], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .unwrap()
    };
    assert_eq!(saved().0, 1);
    for q in held.iter().rev() {
        let lengths = |t: &Trainer| {
            let r = t.snapshot().report.unwrap();
            (r.comparison.recorded.len(), r.comparison.attempt.len())
        };
        let before = lengths(&t);
        let mut partial = reply(q, false, true);
        partial["isDuringSearch"] = serde_json::json!(true);
        t.accept(&partial, 0.5).unwrap();
        assert_eq!(
            lengths(&t),
            before,
            "unfinished estimates cannot complete the chart"
        );
        let mut final_reply = reply(q, false, true);
        // An opponent's final blunder changes the chart, never the grade.
        final_reply["rootInfo"]["winrate"] = serde_json::json!(0.01);
        final_reply["rootInfo"]["scoreLead"] = serde_json::json!(-40.0);
        t.accept(&final_reply, 0.5).unwrap();
        assert_eq!(t.snapshot().phase, Phase::Passed);
    }
    let final_report = t.snapshot().report.unwrap();
    assert_eq!(final_report.comparison.recorded.len(), 11);
    assert_eq!(final_report.comparison.attempt.len(), 11);
    assert_eq!(
        final_report.attempt_loss.winrate,
        report.attempt_loss.winrate
    );
    assert_eq!(final_report.recorded_loss.score, report.recorded_loss.score);
    assert_eq!(final_report.comparison.attempt[10].score, -40.0);
    let (count, json) = saved();
    assert_eq!(
        count, 1,
        "graph completion must not create a second attempt"
    );
    let persisted: katastro::training::Report = serde_json::from_str(&json).unwrap();
    assert_eq!(persisted.comparison.attempt.len(), 11);
    assert_eq!(persisted.comparison.recorded.len(), 11);
}

#[test]
fn early_finish_ignores_work_beyond_its_actual_horizon() {
    let temp = tempfile::tempdir().unwrap();
    let mut t = trainer(temp.path());
    t.begin(
        vec![fixture(temp.path(), false)],
        "me".into(),
        Settings::default(),
    )
    .unwrap();
    discover(&mut t);
    let mut held = vec![];
    drain(&mut t, &mut held, endpoint);
    t.play(None).unwrap();
    drain(&mut t, &mut held, endpoint);
    assert_eq!(t.snapshot().phase, Phase::Failed);
    let before = t.snapshot().report.unwrap();
    assert_eq!(before.comparison.attempt.len(), 3);
    assert!(!held.is_empty(), "fixture must hold unneeded future work");
    for q in held {
        t.accept(&reply(&q, false, true), 0.5).unwrap();
    }
    let after = t.snapshot().report.unwrap();
    assert_eq!(after.comparison.recorded.len(), 3);
    assert_eq!(after.attempt_loss.score, before.attempt_loss.score);
}

#[test]
fn preparation_backfills_an_existing_catalog_without_downgrading_review_values() {
    for indexed in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path(), false);
        let mut review =
            katastro::Review::new(&temp.path().join("reviews"), &temp.path().join("cache"))
                .unwrap();
        review.import(&source).unwrap();
        for node in 0..15 {
            review
                .store_analysis(
                    &review.analysis_key(node, &profile()).unwrap(),
                    &katastro::Analysis {
                        visits: 4096,
                        winrate: if node == 0 { 0.8 } else { 0.4 },
                        score_lead: if node == 0 { 8.0 } else { -2.0 },
                        suggestions: vec![],
                        ownership: vec![],
                        ownership_visits: 0,
                    },
                )
                .unwrap();
        }
        if indexed {
            let mut index = trainer(temp.path());
            index
                .prepare(vec![source.clone()], "me".into(), true)
                .unwrap();
            assert!(
                index.next_request().unwrap().is_none(),
                "review indexing itself does no extra engine work"
            );
            drop(index);
        }
        let mut warm = trainer(temp.path());
        warm.prepare(vec![source.clone()], "me".into(), false)
            .unwrap();
        let requests = settle(&mut warm, false, true);
        assert!(
            requests
                .iter()
                .any(|q| q["moves"].as_array().unwrap().len() == 10),
            "an already indexed candidate still needs its grading window prepared"
        );
        assert!(
            requests.iter().all(|q| q["maxVisits"] == 512),
            "do not redo discovery or lower the grading budget"
        );
        let mut play = trainer(temp.path());
        play.begin(vec![source], "me".into(), Settings::default())
            .unwrap();
        assert!(play.next_request().unwrap().is_none());
        assert_eq!(
            review.cached_analysis(&profile()).unwrap()[&10].visits,
            4096
        );
    }
}

#[test]
fn retry_drops_late_chart_replies_and_keeps_the_new_attempt_blind() {
    let temp = tempfile::tempdir().unwrap();
    let mut t = trainer(temp.path());
    t.begin(
        vec![fixture(temp.path(), false)],
        "me".into(),
        Settings::default(),
    )
    .unwrap();
    settle(&mut t, false, true);
    let mut held = vec![];
    // Row 6 is a losing attempt according to the controlled evaluations.
    for i in 0..5 {
        t.play(Some(Point::new(i, 3))).unwrap();
        drain(&mut t, &mut held, endpoint);
    }
    assert_eq!(t.snapshot().phase, Phase::Failed);
    assert!(!held.is_empty());
    t.retry().unwrap();
    for q in held {
        t.accept(&reply(&q, false, true), 0.5).unwrap();
    }
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
    assert!(t.snapshot().report.is_none());
    assert_eq!(t.snapshot().board.unwrap().move_number, 0);
    for i in 0..5 {
        t.play(Some(Point::new(i, 4))).unwrap();
        settle(&mut t, false, true);
    }
    assert!(t.snapshot().report.unwrap().passed);
    let db = rusqlite::Connection::open(temp.path().join("reviews")).unwrap();
    let reports: Vec<String> = db
        .prepare("SELECT report FROM training_attempts ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(reports.len(), 2);
    let old: katastro::training::Report = serde_json::from_str(&reports[0]).unwrap();
    let new: katastro::training::Report = serde_json::from_str(&reports[1]).unwrap();
    assert!(!old.passed && new.passed);
    assert_eq!(old.comparison.attempt.len(), 10);
    assert_eq!(new.comparison.attempt.len(), 11);
}

#[test]
fn new_attempt_evaluations_take_priority_over_queued_recorded_positions() {
    let temp = tempfile::tempdir().unwrap();
    let mut t = trainer(temp.path());
    t.begin(
        vec![fixture(temp.path(), false)],
        "me".into(),
        Settings::default(),
    )
    .unwrap();
    discover(&mut t);
    let mut held = Vec::new();
    while let Some(q) = t.next_request().unwrap() {
        held.push(q);
    }
    assert_eq!(held.len(), 7);
    t.play(Some(Point::new(0, 4))).unwrap();
    let human = t.next_request().unwrap().unwrap();
    assert_eq!(human["includePolicy"], true);
    t.accept(&reply(&human, false, true), 0.5).unwrap();
    let one = held.pop().unwrap();
    t.accept(&reply(&one, false, true), 0.5).unwrap();
    let attempt = t.next_request().unwrap().unwrap();
    assert_eq!(
        attempt["moves"][0][1], "A5",
        "newly played positions must run ahead of queued background prefetch"
    );
    t.accept(&reply(&attempt, false, true), 0.5).unwrap();
    for q in held {
        t.accept(&reply(&q, false, true), 0.5).unwrap();
    }
    settle(&mut t, false, true);
    for i in 1..5 {
        t.play(Some(Point::new(i, 4))).unwrap();
        settle(&mut t, false, true);
    }
    assert!(t.snapshot().report.unwrap().passed);
}

#[test]
fn choosing_ten_moves_extends_the_default_prepared_window_before_play() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut text = std::fs::read_to_string(&source).unwrap();
    text.pop();
    for i in 14..24 {
        text.push_str(&format!(
            ";{}[{}{}]",
            if i % 2 == 0 { "B" } else { "W" },
            (b'a' + i % 9) as char,
            (b'a' + i / 9) as char
        ));
    }
    text.push(')');
    std::fs::write(&source, text).unwrap();
    let mut background = trainer(temp.path());
    background
        .prepare(vec![source.clone()], "me".into(), false)
        .unwrap();
    settle(&mut background, false, true);
    let mut t = trainer(temp.path());
    t.begin(
        vec![source],
        "me".into(),
        Settings {
            moves: 10,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
    let extra = settle(&mut t, false, true);
    assert!(
        extra
            .iter()
            .any(|q| q["moves"].as_array().unwrap().len() == 20)
    );
    assert!(
        extra
            .iter()
            .all(|q| q["maxVisits"] == 512 && q["moves"].as_array().unwrap().len() > 10)
    );
    for i in 0..10 {
        t.play(Some(Point::new(i % 9, 4 + i / 9))).unwrap();
        settle(&mut t, false, true);
    }
    let report = t.snapshot().report.unwrap();
    assert!(report.passed);
    assert_eq!(report.comparison.attempt.len(), 21);
    assert_eq!(report.comparison.recorded.len(), 21);
}
