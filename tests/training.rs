use katastro::{
    Document, Point,
    training::{Phase, Settings, Trainer},
};
use serde_json::json;
#[path = "support/training.rs"]
mod support;
use support::*;
// Benefit: an AI reply immediately opens the next turn, even with a backlog of
// grading searches. Regression: waiting for evaluations, starving HumanSL, or
// reporting a pass before the held evaluations have actually arrived.
#[test]
fn human_replies_keep_play_moving_while_grading_is_pending() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    let mut held = Vec::new();
    for i in 0..5 {
        t.play(Some(Point::new(i, 4))).unwrap();
        assert!(
            t.play(Some(Point::new(i, 5))).is_err(),
            "no extra move before the opponent replies"
        );
        let human = t
            .next_request()
            .unwrap()
            .expect("grading must leave capacity for the opponent");
        assert_eq!(human["overrideSettings"]["humanSLProfile"], "rank_1d");
        while let Some(q) = t.next_request().unwrap() {
            held.push(q);
        }
        t.accept(&reply(&human, false, true), 0.5).unwrap();
        let state = t.snapshot();
        assert_eq!(
            state.phase,
            if i == 4 {
                Phase::Grading
            } else {
                Phase::YourTurn
            },
            "the next turn must not wait for held grading results"
        );
        assert_eq!(state.board.unwrap().move_number, (i + 1) * 2);
        assert!(state.report.is_none(), "never grade an incomplete attempt");
        while let Some(q) = t.next_request().unwrap() {
            held.push(q);
        }
    }
    assert!(!held.is_empty());
    for q in held.iter().rev() {
        t.accept(&reply(q, false, true), 0.5).unwrap();
    }
    settle(&mut t, false, true);
    let state = t.snapshot();
    assert_eq!(state.phase, Phase::Passed);
    let report = state.report.unwrap();
    assert_eq!(report.attempt_loss.winrate, 0.0);
    assert_eq!(report.attempt_loss.score, 0.0);
    assert!((report.recorded_loss.winrate - 0.4).abs() < 1e-9);
    assert_eq!(report.recorded_loss.score, 10.0);
}
// Benefit: train only on our own loss of a lead, without leaking the recorded move.
// Regression: reversed perspective, opponent mistakes, or a future child in the snapshot.
#[test]
fn library_mining_hides_future_moves_and_uses_the_players_perspective() {
    for white in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path(), white);
        let mut t = trainer(temp.path());
        t.begin(vec![source.clone()], "ME".into(), Settings::default())
            .unwrap();
        settle(&mut t, white, true);
        let s = t.snapshot();
        assert_eq!(
            s.phase,
            Phase::YourTurn,
            "must discover a playable exercise"
        );
        let d = s.document.unwrap();
        let b = d.board(d.selected).unwrap();
        assert_eq!(
            b.next,
            if white {
                katastro::Color::White
            } else {
                katastro::Color::Black
            }
        );
        assert_eq!(b.move_number, usize::from(white));
        assert!(d.nodes[d.selected].children.is_empty());
        assert!(!d.to_sgf().contains("SECRET"));
        assert!(s.report.is_none() && s.reveal.is_none());
        let mut reopened = trainer(temp.path());
        reopened
            .begin(vec![source], "me".into(), Settings::default())
            .unwrap();
        assert!(
            settle(&mut reopened, white, true).is_empty(),
            "reuse persisted analysis across reopen"
        );
        assert_eq!(reopened.snapshot().phase, Phase::YourTurn);
    }
}
// Benefit: failure permits a blind retry, and success unlocks actual explorable lines.
// Regression: reveal on failure, retry retaining stones, or overwriting the played SGF.
#[test]
fn failed_attempt_can_be_retried_blindly_and_given_up_without_changing_source() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let original = std::fs::read(&source).unwrap();
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    for i in 0..5 {
        t.play(Some(Point::new(i, 4))).unwrap();
        settle(&mut t, false, false);
    }
    assert_eq!(t.snapshot().phase, Phase::Failed);
    assert!(t.snapshot().reveal.is_none());
    t.retry().unwrap();
    let d = t.snapshot().document.unwrap();
    assert_eq!(d.board(d.selected).unwrap().move_number, 0);
    assert!(t.snapshot().report.is_none());
    // Fresh random policy replies, but deterministic evaluation reuse for identical positions.
    for i in 0..5 {
        t.play(Some(Point::new(i, 5))).unwrap();
        settle(&mut t, false, true);
    }
    // This fixture only treats row 5 as recovery. Giving up still unlocks the explanation.
    t.reveal().unwrap();
    settle(&mut t, false, true);
    let s = t.snapshot();
    assert_eq!(s.phase, Phase::Revealed);
    let (path, index) = s.reveal.unwrap();
    let d = Document::parse(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(index, 0);
    assert!(d.nodes[d.mainline[index]].children.len() >= 3);
    assert_eq!(
        d.mainline.len(),
        Document::parse(&original).unwrap().mainline.len()
    );
    assert_eq!(std::fs::read(&source).unwrap(), original);
}
#[test]
fn improving_attempt_passes_for_either_color_and_completed_exercise_is_not_repeated() {
    for white in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path(), white);
        let mut t = trainer(temp.path());
        t.begin(vec![source.clone()], "me".into(), Settings::default())
            .unwrap();
        settle(&mut t, white, true);
        for i in 0..5 {
            t.play(Some(Point::new(i, 4))).unwrap();
            let qs = settle(&mut t, white, true);
            assert!(qs.iter().any(
                |q| q["overrideSettings"]["humanSLProfile"] == "rank_1d" && q["maxVisits"] == 1
            ));
            if i < 4 {
                assert_eq!(t.snapshot().phase, Phase::YourTurn);
                assert!(t.snapshot().report.is_none());
            }
        }
        let s = t.snapshot();
        assert_eq!(s.phase, Phase::Passed);
        let r = s.report.unwrap();
        assert!(r.passed && r.winrate_saved() > 0.39 && r.score_saved() > 9.9);
        let mut t = trainer(temp.path());
        t.begin(vec![source], "me".into(), Settings::default())
            .unwrap();
        settle(&mut t, white, true);
        assert_eq!(
            t.snapshot().phase,
            Phase::Idle,
            "skip exercises already passed"
        );
    }
}

#[test]
fn a_lucky_opponent_blunder_does_not_erase_a_major_mistake() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let respond = |q: &serde_json::Value| {
        let mut r = reply(q, false, true);
        let moves = q["moves"].as_array().unwrap();
        let n = moves.len();
        let attempt = moves.iter().any(|m| m[1].as_str().unwrap().ends_with('5'));
        let (wr, score) = if attempt {
            if n == 1 { (0.4, -2.0) } else { (0.8, 8.0) }
        } else {
            match n {
                1 => (0.2, -6.0),
                3 => (0.5, 2.0),
                _ => (0.8, 8.0),
            }
        };
        r["rootInfo"]["winrate"] = json!(wr);
        r["rootInfo"]["scoreLead"] = json!(score);
        r
    };
    for strict in [true, false] {
        let mut t = trainer(temp.path());
        t.begin(
            vec![source.clone()],
            "me".into(),
            Settings {
                strict,
                ..Settings::default()
            },
        )
        .unwrap();
        settle_with(&mut t, respond);
        for i in 0..5 {
            t.play(Some(Point::new(i, 4))).unwrap();
            settle_with(&mut t, respond);
        }
        let r = t.snapshot().report.unwrap();
        assert!(r.winrate_saved() > 0.49 && r.score_saved() > 9.9 && r.major_mistake);
        assert_eq!(
            r.passed, !strict,
            "an overall reduction does not excuse another major mistake when the guard is enabled"
        );
    }
}
#[test]
fn human_policy_is_sampled_proportionally_and_illegal_moves_are_filtered() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    t.play(Some(Point::new(0, 4))).unwrap();
    assert!(
        t.play(Some(Point::new(1, 4))).is_err(),
        "cannot move while the opponent thinks"
    );
    let q = t.next_request().unwrap().unwrap();
    assert_eq!(q["maxVisits"], 1);
    let mut r = reply(&q, false, true);
    let mut policy = vec![-1.0; 82];
    policy[4 * 9] = 1.0; // occupied: never selectable
    policy[4 * 9 + 1] = 0.9;
    policy[4 * 9 + 2] = 0.1;
    r["humanPolicy"] = json!(policy);
    t.accept(&r, 0.95).unwrap();
    settle(&mut t, false, true);
    let d = t.snapshot().document.unwrap();
    assert_eq!(
        d.nodes[d.selected].played.unwrap().point,
        Some(Point::new(2, 4))
    );
    let before = d.to_sgf();
    assert!(t.play(Some(Point::new(0, 4))).is_err());
    assert_eq!(t.snapshot().document.unwrap().to_sgf(), before);
}
#[test]
fn stale_or_partial_replies_cannot_change_the_board_and_give_up_unlocks_only_good_lines() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    t.play(Some(Point::new(0, 4))).unwrap();
    let old = t.next_request().unwrap().unwrap();
    let mut partial = reply(&old, false, true);
    partial["isDuringSearch"] = json!(true);
    t.accept(&partial, 0.5).unwrap();
    assert_eq!(
        t.snapshot().document.unwrap().board(1).unwrap().move_number,
        1
    );
    t.reveal().unwrap();
    let before = t.snapshot().document.unwrap().to_sgf();
    t.accept(&reply(&old, false, true), 0.5).unwrap();
    assert_eq!(
        t.snapshot().document.unwrap().to_sgf(),
        before,
        "a canceled AI move cannot alter the attempt"
    );
    settle(&mut t, false, true);
    assert_eq!(
        t.snapshot().lines.len(),
        2,
        "present only the two plausible lines"
    );
    let (path, index) = t.snapshot().reveal.unwrap();
    let d = Document::parse(&std::fs::read(&path).unwrap()).unwrap();
    let anchor = d.mainline[index];
    let first_moves: Vec<_> = d.nodes[anchor]
        .children
        .iter()
        .map(|id| d.nodes[*id].played.unwrap().point)
        .collect();
    assert!(
        first_moves.contains(&Some(Point::new(3, 4)))
            && first_moves.contains(&Some(Point::new(4, 4)))
    );
    assert!(
        !first_moves.contains(&Some(Point::new(0, 8))),
        "reject a searched but clearly worse line"
    );
    let mut review =
        katastro::Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&path).unwrap();
    review.select(anchor).unwrap();
    let branch = review.play(Some(Point::new(8, 4))).unwrap();
    drop(review);
    let mut review =
        katastro::Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&path).unwrap();
    assert_eq!(
        review.document().unwrap().nodes[branch]
            .played
            .unwrap()
            .point,
        Some(Point::new(8, 4))
    );
}
#[test]
fn two_passes_grade_at_the_matching_recorded_ply_instead_of_the_full_horizon() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    let respond = |q: &serde_json::Value| {
        let mut r = reply(q, false, true);
        if q["moves"].as_array().unwrap().len() == 3 {
            r["rootInfo"]["winrate"] = json!(0.2);
            r["rootInfo"]["scoreLead"] = json!(-8.0);
        }
        r
    };
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    settle_with(&mut t, respond);
    t.play(None).unwrap();
    let requests = settle_with(&mut t, respond);
    assert_eq!(t.snapshot().phase, Phase::Failed);
    assert_eq!(t.snapshot().played, 1);
    assert!(
        requests
            .iter()
            .all(|q| q["moves"].as_array().unwrap().len() <= 2),
        "ignore recorded mistakes beyond the early finish"
    );
    let r = t.snapshot().report.unwrap();
    assert!((r.recorded_loss.winrate - 0.4).abs() < 1e-9);
    assert!((r.recorded_loss.score - 10.0).abs() < 1e-9);
    assert!((r.attempt_loss.winrate - 0.4).abs() < 1e-9);
}
#[test]
fn screening_never_substitutes_for_confirmation_and_cache_identity_includes_the_model() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    while let Some(q) = t.next_request().unwrap() {
        let mut r = reply(&q, false, true);
        if q["maxVisits"] == 512 {
            r["rootInfo"]["winrate"] = json!(0.5);
            r["rootInfo"]["scoreLead"] = json!(0.0);
        }
        t.accept(&r, 0.5).unwrap();
    }
    assert_eq!(
        t.snapshot().phase,
        Phase::Idle,
        "a shallow apparent swing is not an exercise"
    );
    let mut other = profile();
    other.model_sha256 = "new-model".into();
    let mut t = Trainer::new(
        &temp.path().join("reviews"),
        &temp.path().join("cache"),
        &temp.path().join("Training"),
        other,
    )
    .unwrap();
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    assert!(!settle(&mut t, false, true).is_empty());
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
}

#[test]
fn completed_exercises_can_be_practiced_again_when_requested() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    t.reveal().unwrap();
    settle(&mut t, false, true);
    t.begin(
        vec![source],
        "me".into(),
        Settings {
            include_completed: true,
            ..Settings::default()
        },
    )
    .unwrap();
    settle(&mut t, false, true);
    assert_eq!(
        t.snapshot().phase,
        Phase::YourTurn,
        "a given-up or passed exercise can be requested again"
    );
}

#[test]
fn confirmed_results_with_actual_visits_above_the_requested_budget_are_reused() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    while let Some(q) = t.next_request().unwrap() {
        let mut r = reply(&q, false, true);
        r["rootInfo"]["visits"] = json!(q["maxVisits"].as_u64().unwrap() + 1);
        t.accept(&r, 0.5).unwrap();
    }
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
    drop(t);
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    assert!(
        settle(&mut t, false, true).is_empty(),
        "requested budget is part of the training cache identity; actual effort can be slightly higher"
    );
    assert_eq!(t.snapshot().phase, Phase::YourTurn);
}

#[test]
fn training_progress_is_scoped_to_the_named_player() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    t.reveal().unwrap();
    settle(&mut t, false, true);
    let other = temp.path().join("other-player.sgf");
    std::fs::write(
        &other,
        std::fs::read_to_string(&source)
            .unwrap()
            .replace("PB[me]", "PB[another]"),
    )
    .unwrap();
    t.begin(vec![other], "another".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    assert_eq!(
        t.snapshot().phase,
        Phase::YourTurn,
        "another player's progress must not hide this exercise"
    );
}

#[test]
fn clearing_analysis_recomputes_training_values_but_preserves_completed_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    t.reveal().unwrap();
    settle(&mut t, false, true);
    drop(t);
    let mut review =
        katastro::Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.clear_analysis().unwrap();
    drop(review);
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    assert!(
        !settle(&mut t, false, true).is_empty(),
        "clearing analysis must evict training evaluations too"
    );
    assert_eq!(
        t.snapshot().phase,
        Phase::Idle,
        "the completed attempt must survive cache eviction"
    );
}

#[test]
fn another_attempt_preserves_exploratory_variations_from_the_previous_reveal() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source.clone()], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    t.reveal().unwrap();
    settle(&mut t, false, true);
    let (path, index) = t.snapshot().reveal.unwrap();
    let mut review =
        katastro::Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&path).unwrap();
    let anchor = review.document().unwrap().mainline[index];
    review.select(anchor).unwrap();
    review.play(Some(Point::new(8, 4))).unwrap();
    drop(review);
    t.begin(
        vec![source],
        "me".into(),
        Settings {
            include_completed: true,
            ..Settings::default()
        },
    )
    .unwrap();
    settle(&mut t, false, true);
    t.play(Some(Point::new(2, 4))).unwrap();
    settle(&mut t, false, true);
    t.reveal().unwrap();
    settle(&mut t, false, true);
    let mut review =
        katastro::Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&path).unwrap();
    let d = review.document().unwrap();
    let anchor = d.mainline[index];
    assert!(
        d.nodes[anchor]
            .children
            .iter()
            .any(|id| d.nodes[*id].played.unwrap().point == Some(Point::new(8, 4))),
        "new training attempts must retain the user's earlier exploration"
    );
    assert!(
        d.nodes[anchor]
            .children
            .iter()
            .any(|id| d.nodes[*id].played.unwrap().point == Some(Point::new(2, 4))),
        "the new attempt should also be present"
    );
}

// Repeated opponent mistakes must neither cancel our losses nor make the
// recorded continuation look like a clean baseline.
#[test]
fn reducing_cumulative_own_move_losses_passes_even_when_the_opponent_restored_the_recorded_lead() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    let respond = |q: &serde_json::Value| {
        let mut r = reply(q, false, true);
        let moves = q["moves"].as_array().unwrap();
        let n = moves.len();
        let attempt = moves.iter().any(|m| m[1].as_str().unwrap().ends_with('5'));
        let (wr, score) = if attempt {
            // Each of our moves loses 5 pp and 1 point. Opponent replies are
            // deliberately strong; their effect is not our move quality.
            let ours = n.div_ceil(2) as f64;
            let theirs = (n / 2) as f64;
            (0.8 - 0.05 * ours - 0.04 * theirs, 8.0 - ours - theirs)
        } else {
            match n {
                1 => (0.4, -2.0),
                3 => (0.6, 4.0),
                _ => (0.8, 8.0),
            }
        };
        r["rootInfo"]["winrate"] = json!(wr);
        r["rootInfo"]["scoreLead"] = json!(score);
        r
    };
    while let Some(q) = t.next_request().unwrap() {
        t.accept(&respond(&q), 0.5).unwrap();
    }
    for i in 0..5 {
        t.play(Some(Point::new(i, 4))).unwrap();
        while let Some(q) = t.next_request().unwrap() {
            t.accept(&respond(&q), 0.5).unwrap();
        }
    }
    assert_eq!(
        t.snapshot().phase,
        Phase::Passed,
        "25 pp of our losses improves on 60 pp, despite a worse final evaluation"
    );
    let r = t.snapshot().report.unwrap();
    assert!(
        (r.attempt_loss.winrate - 0.25).abs() < 1e-9 && (r.attempt_loss.score - 5.0).abs() < 1e-9,
        "opponent-caused drops are excluded"
    );
    assert!(
        (r.recorded_loss.winrate - 0.6).abs() < 1e-9 && (r.recorded_loss.score - 14.0).abs() < 1e-9
    );
}
#[test]
fn many_small_player_mistakes_cannot_be_paid_back_by_ai_blunders() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    for i in 0..5 {
        t.play(Some(Point::new(i, 4))).unwrap();
        while let Some(q) = t.next_request().unwrap() {
            let mut r = reply(&q, false, true);
            let moves = q["moves"].as_array().unwrap();
            if moves.iter().any(|m| m[1].as_str().unwrap().ends_with('5')) {
                // Five individually sub-threshold mistakes total 50 pp.
                // Every AI reply gives the entire loss back.
                let (wr, score) = if moves.len() % 2 == 1 {
                    (0.7, 6.0)
                } else {
                    (0.8, 8.0)
                };
                r["rootInfo"]["winrate"] = json!(wr);
                r["rootInfo"]["scoreLead"] = json!(score);
            }
            t.accept(&r, 0.5).unwrap();
        }
    }
    assert_eq!(
        t.snapshot().phase,
        Phase::Failed,
        "our 50 pp loss is worse than the recorded 40 pp; the AI's gifts do not count"
    );
    assert!(
        !t.snapshot().report.unwrap().major_mistake,
        "the cumulative criterion must catch repeated smaller mistakes too"
    );
}

#[test]
fn good_player_moves_do_not_cancel_earlier_losses_and_each_metric_is_accumulated_separately() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path(), false);
    let mut t = trainer(temp.path());
    t.begin(vec![source], "me".into(), Settings::default())
        .unwrap();
    settle(&mut t, false, true);
    for i in 0..5 {
        t.play(Some(Point::new(i, 4))).unwrap();
        settle_with(&mut t, |q| {
            let mut r = reply(q, false, true);
            let moves = q["moves"].as_array().unwrap();
            if moves.iter().any(|m| m[1].as_str().unwrap().ends_with('5')) {
                let (wr, score) = if moves.len() <= 2 {
                    (0.7, 7.0)
                } else if moves.len() <= 4 {
                    (0.8, 8.0)
                } else {
                    (0.8, 7.0)
                };
                r["rootInfo"]["winrate"] = json!(wr);
                r["rootInfo"]["scoreLead"] = json!(score);
            }
            r
        });
    }
    let r = t.snapshot().report.unwrap();
    assert!(r.passed);
    assert!(
        (r.attempt_loss.winrate - 0.1).abs() < 1e-9,
        "the later winrate gain must not cancel the first loss"
    );
    assert!(
        (r.attempt_loss.score - 2.0).abs() < 1e-9,
        "positive score changes do not cancel earlier losses; a score loss also counts when winrate is flat"
    );
}
