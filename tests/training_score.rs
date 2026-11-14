use katastro::{
    Color, Document, Point, digest,
    training::{Phase, Settings},
};
use serde_json::{Value, json};
#[path = "support/training.rs"]
mod support;
use support::*;

fn evaluated(q: &Value, white: bool, score: f64, winrate: f64) -> Value {
    let mut r = reply(q, white, true);
    r["rootInfo"]["scoreLead"] = json!(if white { -score } else { score });
    r["rootInfo"]["winrate"] = json!(if white { 1.0 - winrate } else { winrate });
    r
}

// Benefit: train meaningful point losses, including from losing positions.
// Regression: winrate gates, reversed White scores, or an exclusive boundary.
#[test]
fn five_point_losses_qualify_even_when_behind_and_winrate_is_flat() {
    for white in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut t = trainer(temp.path());
        t.begin(
            vec![fixture(temp.path(), white)],
            "me".into(),
            Settings::default(),
        )
        .unwrap();
        settle_with(&mut t, |q| {
            let after = q["moves"].as_array().unwrap().len() > usize::from(white);
            evaluated(q, white, if after { -15.0 } else { -10.0 }, 0.01)
        });
        assert_eq!(t.snapshot().phase, Phase::YourTurn);
        assert_eq!(t.snapshot().board.unwrap().move_number, usize::from(white));
    }
}

// Regression: a probability collapse or coarse screening bypasses confirmation.
#[test]
fn less_than_five_points_is_rejected_despite_a_winrate_collapse() {
    for white in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut t = trainer(temp.path());
        t.begin(
            vec![fixture(temp.path(), white)],
            "me".into(),
            Settings::default(),
        )
        .unwrap();
        settle_with(&mut t, |q| {
            let after = q["moves"].as_array().unwrap().len() > usize::from(white);
            let loss = if q["maxVisits"] == 64 { 8.0 } else { 4.99 };
            evaluated(
                q,
                white,
                if after { 8.0 - loss } else { 8.0 },
                if after { 0.01 } else { 0.99 },
            )
        });
        assert_eq!(t.snapshot().phase, Phase::Idle);
    }
}

#[test]
fn largest_point_loss_is_selected_ahead_of_largest_winrate_loss() {
    let temp = tempfile::tempdir().unwrap();
    let mut t = trainer(temp.path());
    t.begin(
        vec![fixture(temp.path(), false)],
        "me".into(),
        Settings::default(),
    )
    .unwrap();
    settle_with(&mut t, |q| {
        let (score, wr) = match q["moves"].as_array().unwrap().len() {
            1 => (2.0, 0.2),
            3 => (-2.0, 0.6),
            _ => (8.0, 0.8),
        };
        evaluated(q, false, score, wr)
    });
    assert_eq!(t.snapshot().board.unwrap().move_number, 2);
}

// Benefit: grade only our accumulated negative point changes. Winrate cannot
// rescue an unimproved attempt, veto an improvement, or trigger the guard.
#[test]
fn grades_and_major_mistakes_depend_only_on_points() {
    for (loss, wr, strict, passed, major) in [
        (0.0, 0.01, true, true, false),
        (4.99, 0.01, true, true, false),
        (5.0, 0.99, true, false, true),
        (5.0, 0.01, false, true, true),
        (7.0, 0.01, false, true, true),
        (7.01, 0.99, false, false, true),
        (10.0, 0.99, false, false, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut t = trainer(temp.path());
        t.begin(
            vec![fixture(temp.path(), false)],
            "me".into(),
            Settings {
                strict,
                ..Default::default()
            },
        )
        .unwrap();
        settle(&mut t, false, true);
        for i in 0..5 {
            t.play(Some(Point::new(i, 4))).unwrap();
            settle_with(&mut t, |q| evaluated(q, false, 8.0 - loss, wr));
        }
        let r = t.snapshot().report.unwrap();
        assert_eq!(
            r.passed, passed,
            "loss={loss}, winrate={wr}, strict={strict}"
        );
        assert_eq!(r.major_mistake, major, "loss={loss}");
        assert!((r.attempt_loss.score - loss).abs() < 1e-9);
        assert_eq!(r.recorded_loss.score, 10.0);
        let saved: String = rusqlite::Connection::open(temp.path().join("reviews"))
            .unwrap()
            .query_row(
                "SELECT report FROM training_attempts ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!saved.contains("winrate"), "persist score-only reports");
    }
}

// Benefit: explore plausible lines even where probability estimates disagree.
#[test]
fn suggested_lines_use_score_quality_without_winrate_filtering() {
    for white in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut t = trainer(temp.path());
        t.begin(
            vec![fixture(temp.path(), white)],
            "me".into(),
            Settings::default(),
        )
        .unwrap();
        settle(&mut t, white, true);
        t.reveal().unwrap();
        settle_with(&mut t, |q| {
            let mut r = reply(q, white, true);
            if let Some(infos) = r["moveInfos"].as_array_mut() {
                infos[1]["winrate"] = json!(if white { 0.99 } else { 0.01 });
                infos[2]["winrate"] = json!(if white { 0.0 } else { 1.0 });
            }
            r
        });
        assert_eq!(t.snapshot().lines.len(), 2);
        assert_eq!(t.snapshot().lines[1][0], Some(Point::new(4, 4)));
    }
}

// Benefit: upgrading recalculates both old positive/negative indexes and grades,
// while retaining historical attempts and reusing expensive position analysis.
#[test]
fn legacy_winrate_catalogs_and_completion_do_not_override_score_policy() {
    for old_candidate in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path(), false);
        let doc = Document::parse(&std::fs::read(&source).unwrap()).unwrap();
        let mut t = trainer(temp.path());
        let old_catalog = digest(
            &serde_json::to_vec(&(
                "training-candidates-v1",
                doc.source_identity().unwrap(),
                &doc.mainline,
                "me",
                Some(Color::Black),
                profile(),
            ))
            .unwrap(),
        );
        rusqlite::Connection::open(temp.path().join("cache"))
            .unwrap()
            .execute(
                "INSERT INTO training_games(key,result) VALUES (?1,?2)",
                rusqlite::params![
                    old_catalog,
                    json!({"candidates":if old_candidate {vec![0]} else {vec![]},"complete":true})
                        .to_string()
                ],
            )
            .unwrap();
        let old_problem = digest(
            &serde_json::to_vec(&(
                "me",
                doc.source_identity().unwrap(),
                doc.position(doc.mainline[0]).unwrap(),
                profile(),
            ))
            .unwrap(),
        );
        let db = rusqlite::Connection::open(temp.path().join("reviews")).unwrap();
        db.execute("INSERT INTO training_attempts(problem,outcome,report,document) VALUES (?1,'passed','null','null')", [&old_problem]).unwrap();
        t.begin(vec![source.clone()], "me".into(), Settings::default())
            .unwrap();
        settle_with(&mut t, |q| {
            let after = !q["moves"].as_array().unwrap().is_empty();
            evaluated(
                q,
                false,
                if after && !old_candidate { -2.0 } else { 8.0 },
                if after { 0.4 } else { 0.8 },
            )
        });
        assert_eq!(
            t.snapshot().phase,
            if old_candidate {
                Phase::Idle
            } else {
                Phase::YourTurn
            }
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM training_attempts WHERE problem=?1",
                [&old_problem],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        drop(t);
        let mut t = trainer(temp.path());
        t.begin(vec![source], "me".into(), Settings::default())
            .unwrap();
        assert!(
            t.next_request().unwrap().is_none(),
            "restart reuses score-policy discovery and evaluations"
        );
        assert_eq!(
            t.snapshot().phase,
            if old_candidate {
                Phase::Idle
            } else {
                Phase::YourTurn
            }
        );
    }
}
