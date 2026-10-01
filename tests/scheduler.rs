use katastro::{Analysis, Document, EngineProfile, Point, Review, scheduler::Scheduler};
use serde_json::json;
use std::collections::BTreeMap;
fn profile() -> EngineProfile {
    EngineProfile {
        model_sha256: "a".into(),
        engine_sha256: "b".into(),
        settings_digest: "c".into(),
    }
}
fn doc() -> Document {
    Document::parse(b"(;SZ[9];B[cc];W[gg];B[cg])").unwrap()
}
fn reply(id: &str, turn: usize, visits: u64, partial: bool) -> serde_json::Value {
    json!({"id":id,"turnNumber":turn,"isDuringSearch":partial,"rootInfo":{"visits":visits,"winrate":0.0,"scoreLead":0.0}})
}
#[test]
fn cheap_coverage_arrives_out_of_order_before_deep_work_and_survives_reopen() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc];W[gg];B[cg])").unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    let mut scheduler = Scheduler::new(1, doc(), profile(), BTreeMap::new());
    let cheap = scheduler
        .next_request()
        .unwrap()
        .expect("missing positions must be analyzed");
    assert_eq!(cheap.visits, 1);
    assert_eq!(cheap.query["analyzeTurns"], json!([0, 1, 2, 3]));
    assert!(scheduler.next_request().unwrap().is_none());
    for turn in [3, 1, 0] {
        for (target, value) in scheduler.accept(&reply(&cheap.id, turn, 1, false)).unwrap() {
            review.store_analysis(&target.key, &value).unwrap();
        }
    }
    assert_eq!(scheduler.coverage(), (3, 4));
    assert!(
        scheduler.next_request().unwrap().is_none(),
        "refinement must wait for missing coverage"
    );
    for (target, value) in scheduler.accept(&reply(&cheap.id, 2, 1, false)).unwrap() {
        review.store_analysis(&target.key, &value).unwrap();
    }
    assert_eq!(scheduler.coverage(), (4, 4));
    assert!(review.cached_analysis(&profile()).unwrap().is_empty());
    for budget in [8, 64] {
        let deep = scheduler.next_request().unwrap().unwrap();
        assert_eq!(deep.visits, budget);
        for turn in [3, 1, 0, 2] {
            for (target, value) in scheduler
                .accept(&reply(&deep.id, turn, budget, false))
                .unwrap()
            {
                review.store_analysis(&target.key, &value).unwrap();
            }
        }
    }
    assert_eq!(scheduler.values[&2].winrate, 0.0);
    assert_eq!(scheduler.values[&2].score_lead, 0.0);
    drop(review);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&source).unwrap();
    assert_eq!(reopened.cached_analysis(&profile()).unwrap().len(), 4);
    let mut from_cache = Scheduler::new(
        2,
        doc(),
        profile(),
        reopened.cached_analysis(&profile()).unwrap(),
    );
    assert_eq!(
        from_cache.next_request().unwrap().unwrap().visits,
        64,
        "reopen reuses the chart while backfilling legacy ownership with a small request"
    );
}
#[test]
fn selecting_a_new_variation_interrupts_deep_work_and_ignores_stale_replies() {
    let mut cached = BTreeMap::new();
    for node in 0..4 {
        cached.insert(
            node,
            Analysis {
                ownership: vec![],
                ownership_visits: 0,
                visits: 8,
                winrate: 0.5,
                score_lead: 0.0,
                suggestions: vec![],
            },
        );
    }
    let mut scheduler = Scheduler::new(7, doc(), profile(), cached);
    let deep = scheduler.next_request().unwrap().unwrap();
    assert_eq!(deep.visits, 64);
    let mut changed = doc();
    changed.selected = 1;
    let branch = changed.append_move(Some(Point::new(3, 3))).unwrap();
    assert!(scheduler.select(changed).contains(&deep.id));
    let selected = scheduler.next_request().unwrap().unwrap();
    assert_eq!(selected.visits, 1);
    assert_eq!(
        selected.query["includeOwnership"], true,
        "a newly selected variation should receive ownership in its first one-visit response"
    );
    assert_eq!(selected.query["priority"], 100);
    assert_eq!(
        selected.targets.values().flatten().next().unwrap().node,
        branch
    );
    scheduler.accept(&reply(&selected.id, 2, 1, true)).unwrap();
    assert_eq!(scheduler.values[&branch].visits, 1);
    assert!(scheduler.pending() > 0);
    scheduler
        .accept(&json!({"id":"ack","action":"terminate","terminateId":selected.id}))
        .unwrap();
    assert!(scheduler.pending() > 0, "termination ack is not completion");
    scheduler.accept(&reply(&selected.id, 2, 1, false)).unwrap();
    assert_eq!(scheduler.pending(), 0);
    let before = scheduler.values.clone();
    scheduler
        .accept(&reply("old-generation", 2, 999, false))
        .unwrap();
    assert_eq!(scheduler.values, before);
    scheduler.accept(&reply(&deep.id, 2, 999, false)).unwrap();
    assert_eq!(
        scheduler.values, before,
        "canceled work must not modify the selected review"
    );
}
#[test]
fn successive_variation_moves_get_scored_hints_while_older_quick_requests_are_pending() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc])").unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    let mut scheduler = Scheduler::new(
        1,
        review.document().unwrap().clone(),
        profile(),
        BTreeMap::new(),
    );
    let coverage = scheduler.next_request().unwrap().unwrap();
    review.play(Some(Point::new(3, 3))).unwrap();
    scheduler.select(review.document().unwrap().clone());
    let previous = scheduler.next_request().unwrap().unwrap();
    assert_eq!(previous.visits, 1);
    let selected = review.play(Some(Point::new(4, 4))).unwrap();
    let canceled = scheduler.select(review.document().unwrap().clone());
    let current = scheduler.next_request().unwrap().expect(
        "the newest variation must be analyzed without waiting for superseded quick work or whole-game coverage",
    );
    assert!(canceled.contains(&previous.id));
    assert!(
        !canceled.contains(&coverage.id),
        "original-game coverage must continue"
    );
    assert_eq!(
        current.targets.values().flatten().next().unwrap().node,
        selected
    );
    assert_eq!(current.query["priority"], 100);
    let old_turn = *previous.targets.keys().next().unwrap();
    scheduler.accept(&json!({"id":previous.id, "turnNumber":old_turn, "isDuringSearch":false, "noResults":true})).unwrap();
    let turn = *current.targets.keys().next().unwrap();
    let mut policy = reply(&current.id, turn, 1, false);
    policy["ownership"] = json!(vec![0.0; 81]);
    policy["policy"] = json!(vec![1.0 / 82.0; 82]);
    scheduler.accept(&policy).unwrap();
    let searched = scheduler
        .next_request()
        .unwrap()
        .expect("a policy-only variation result must be followed by scored hints");
    let mut result = reply(&searched.id, turn, 64, false);
    result["ownership"] = json!(vec![0.1; 81]);
    result["moveInfos"] = json!([{"move":"F5", "visits":32, "winrate":0.6, "scoreLead":1.5}]);
    for (target, value) in scheduler.accept(&result).unwrap() {
        review.store_analysis(&target.key, &value).unwrap();
    }
    assert_eq!(
        scheduler.coverage(),
        (0, 2),
        "interactive hints must not depend on finishing the original chart"
    );
    assert_eq!(
        scheduler.values[&selected].suggestions[0].point,
        Some(Point::new(5, 4))
    );
    assert!(
        scheduler.values[&selected].suggestions[0]
            .score_lead
            .is_some()
    );
    for turn in [0, 1] {
        scheduler
            .accept(&reply(&coverage.id, turn, 1, false))
            .unwrap();
    }
    assert_eq!(
        scheduler.coverage(),
        (2, 2),
        "the still-running original chart must finish using its original request"
    );
    review
        .play(scheduler.values[&selected].suggestions[0].point)
        .unwrap();
    drop(review);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&source).unwrap();
    let doc = reopened.document().unwrap();
    assert_eq!(doc.mainline, vec![0, 1]);
    assert_eq!(
        doc.board(doc.selected).unwrap().stone(Point::new(5, 4)),
        Some(katastro::Color::Black)
    );
    assert_eq!(
        reopened.cached_analysis(&profile()).unwrap()[&selected].suggestions,
        scheduler.values[&selected].suggestions
    );
}
#[test]
fn no_results_and_lower_visit_updates_do_not_corrupt_the_chart() {
    let mut scheduler = Scheduler::new(1, doc(), profile(), BTreeMap::new());
    let cheap = scheduler.next_request().unwrap().unwrap();
    scheduler
        .accept(&json!({"id":cheap.id,"turnNumber":0,"isDuringSearch":false,"noResults":true}))
        .unwrap();
    assert!(!scheduler.values.contains_key(&0));
    scheduler.accept(&reply(&cheap.id, 1, 64, true)).unwrap();
    scheduler.accept(&reply(&cheap.id, 1, 1, false)).unwrap();
    assert_eq!(scheduler.values[&1].visits, 64);
    for turn in [2, 3] {
        scheduler.accept(&reply(&cheap.id, turn, 1, false)).unwrap();
    }
    assert_eq!(scheduler.coverage(), (3, 4));
    assert_eq!(scheduler.pending(), 0);
    assert_eq!(
        scheduler.next_request().unwrap().unwrap().visits,
        8,
        "a failed position must not prevent refining usable data"
    );
}

#[test]
fn refinement_continues_past_64_and_includes_saved_variations() {
    let mut document = doc();
    document.selected = 1;
    let branch = document.append_move(Some(Point::new(3, 3))).unwrap();
    let mut scheduler = Scheduler::new(4, document, profile(), BTreeMap::new());
    for budget in [1, 8, 64, 256, 1024, 4096] {
        while scheduler.values.len() < 5
            || scheduler.values.values().any(|value| value.visits < budget)
        {
            let request = scheduler
                .next_request()
                .unwrap()
                .expect("refinement stopped");
            assert!(
                request.visits == budget
                    || (budget == 8
                        && request.visits == 64
                        && request.targets.values().flatten().all(|t| t.node == branch))
            );
            for turn in request.targets.keys() {
                scheduler
                    .accept(&reply(&request.id, *turn, request.visits, false))
                    .unwrap();
            }
        }
        assert!(scheduler.values.contains_key(&branch));
        assert!(
            scheduler
                .values
                .values()
                .all(|value| value.visits >= budget)
        );
        assert!(
            !scheduler.is_complete(),
            "analysis must remain active after a pass"
        );
    }
}

#[test]
fn engine_candidate_ranking_coordinates_and_cache_survive_reopen() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[19];B[cc])").unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    let mut scheduler = Scheduler::new(
        1,
        review.document().unwrap().clone(),
        profile(),
        BTreeMap::new(),
    );
    let request = scheduler.next_request().unwrap().unwrap();
    let mut value = reply(&request.id, 0, 256, false);
    value["moveInfos"] = json!([
        {"move":"pass","order":2,"visits":0,"winrate":0.0,"scoreLead":-7.5},
        {"move":"J16","order":0,"visits":180,"winrate":0.6,"scoreLead":3.5},
        {"move":"T1","order":1,"visits":60,"winrate":0.55,"scoreLead":1.5},
        {"move":"A19","order":3,"visits":4,"winrate":0.5,"scoreLead":0.0},
        {"move":"D4","order":4,"visits":3,"winrate":0.4,"scoreLead":-1.0},
        {"move":"Q4","order":5,"visits":2,"winrate":0.3,"scoreLead":-2.0}
    ]);
    let accepted = scheduler.accept(&value).unwrap();
    assert_eq!(scheduler.values[&0].suggestions.len(), 6);
    assert_eq!(
        scheduler.values[&0].suggestions[0].point,
        Some(Point::new(8, 3))
    );
    assert_eq!(
        scheduler.values[&0].suggestions[1].point,
        Some(Point::new(18, 18))
    );
    assert_eq!(scheduler.values[&0].suggestions[2].point, None);
    assert_eq!(
        scheduler.values[&0].suggestions[5].point,
        Some(Point::new(15, 15))
    );
    for (target, value) in accepted {
        review.store_analysis(&target.key, &value).unwrap();
    }
    drop(review);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&source).unwrap();
    assert_eq!(
        reopened.cached_analysis(&profile()).unwrap()[&0],
        scheduler.values[&0]
    );
}

#[test]
fn one_visit_policy_preview_provides_legal_ranked_moves_before_search() {
    let mut scheduler = Scheduler::new(1, doc(), profile(), BTreeMap::new());
    let request = scheduler.next_request().unwrap().unwrap();
    assert_eq!(
        request.query["includePolicy"], true,
        "one visit can have no searched moves"
    );
    let mut value = reply(&request.id, 0, 1, false);
    let mut policy = vec![-1.0; 82];
    policy[3 * 9 + 3] = 0.6;
    policy[4 * 9 + 4] = 0.2;
    policy[81] = 0.1;
    for (index, probability) in [(0, 0.04), (1, 0.03), (2, 0.02), (3, 0.01), (4, 0.0)] {
        policy[index] = probability;
    }
    value["moveInfos"] = json!([]);
    value["policy"] = json!(policy);
    scheduler.accept(&value).unwrap();
    let suggestions = &scheduler.values[&0].suggestions;
    assert_eq!(
        suggestions.len(),
        8,
        "legal policy previews must not be truncated to five"
    );
    assert_eq!(suggestions[0].point, Some(Point::new(3, 3)));
    assert_eq!(suggestions[1].point, Some(Point::new(4, 4)));
    assert_eq!(suggestions[2].point, None);
    assert_eq!(suggestions[7].point, Some(Point::new(4, 0)));
    assert!(
        suggestions
            .iter()
            .all(|s| s.visits == 0 && s.winrate.is_none() && s.score_lead.is_none())
    );
}

#[test]
fn ownership_refines_with_the_position_and_reopens_without_losing_deeper_results() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9]AB[bb]AW[gg];B[cc])").unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    let mut scheduler = Scheduler::new(
        1,
        review.document().unwrap().clone(),
        profile(),
        BTreeMap::new(),
    );
    let cheap = scheduler.next_request().unwrap().unwrap();
    assert_eq!(cheap.visits, 1);
    assert_eq!(
        cheap.query["includeOwnership"], false,
        "the initial chart pass must remain cheap"
    );
    for turn in [0, 1] {
        scheduler.accept(&reply(&cheap.id, turn, 1, false)).unwrap();
    }
    for visits in [8, 64, 256] {
        let request = scheduler.next_request().unwrap().unwrap();
        assert_eq!(request.visits, visits);
        assert_eq!(
            request.query["includeOwnership"], true,
            "refinement must request group estimates"
        );
        for turn in [0, 1] {
            let mut value = reply(&request.id, turn, visits, false);
            let mut ownership = vec![0.0; 81];
            ownership[10] = if visits == 256 { -0.9 } else { 0.9 };
            ownership[60] = -0.95;
            value["ownership"] = json!(ownership);
            let accepted = scheduler.accept(&value).unwrap();
            assert_eq!(
                accepted[0].1.ownership, ownership,
                "ownership must reach the selected position, not be discarded"
            );
            review
                .store_analysis(&accepted[0].0.key, &accepted[0].1)
                .unwrap();
        }
        if visits == 8 {
            assert!(review.cached_analysis(&profile()).unwrap().is_empty());
        }
    }
    let expected = scheduler.values.clone();
    let key = review.analysis_key(0, &profile()).unwrap();
    let mut shallower = expected[&0].clone();
    shallower.visits = 64;
    shallower.ownership_visits = 64;
    shallower.ownership.fill(1.0);
    review.store_analysis(&key, &shallower).unwrap();
    drop(review);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&source).unwrap();
    let cached = reopened.cached_analysis(&profile()).unwrap();
    assert_eq!(
        cached, expected,
        "offline reopen must restore the deepest ownership and charts together"
    );
    let mut scheduler = Scheduler::new(2, reopened.document().unwrap().clone(), profile(), cached);
    assert_eq!(
        scheduler.next_request().unwrap().unwrap().visits,
        1024,
        "cached ownership must not trigger repeated cheap analysis"
    );
}

#[test]
fn malformed_ownership_cannot_replace_a_usable_position_evaluation() {
    for ownership in [
        json!([0.0]),
        json!(vec![1.1; 81]),
        json!(["bad"]),
        serde_json::Value::Null,
    ] {
        let mut scheduler = Scheduler::new(1, doc(), profile(), BTreeMap::new());
        let cheap = scheduler.next_request().unwrap().unwrap();
        for turn in 0..4 {
            scheduler.accept(&reply(&cheap.id, turn, 1, false)).unwrap();
        }
        let request = scheduler.next_request().unwrap().unwrap();
        scheduler.accept(&reply(&request.id, 0, 4, true)).unwrap();
        let usable = scheduler.values[&0].clone();
        let mut invalid = reply(&request.id, 0, 8, false);
        invalid["ownership"] = ownership;
        assert!(
            scheduler.accept(&invalid).is_err(),
            "malformed ownership should be rejected, not displayed as group strength"
        );
        assert_eq!(
            scheduler.values[&0], usable,
            "bad ownership must leave the existing chart evaluation usable"
        );
    }
}

#[test]
fn missing_group_map_gets_a_bounded_selected_request_and_does_not_downgrade_deep_charts() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("game.sgf");
    std::fs::write(&source, b"(;SZ[9];B[bb];W[gg])").unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    review.select(1).unwrap();
    let deep: Analysis =
        serde_json::from_value(json!({"visits":16384,"winrate":0.73,"score_lead":7.25})).unwrap();
    let key = review.analysis_key(1, &profile()).unwrap();
    review.store_analysis(&key, &deep).unwrap();
    let mut scheduler = Scheduler::new(
        1,
        review.document().unwrap().clone(),
        profile(),
        review.cached_analysis(&profile()).unwrap(),
    );
    let quick = scheduler.next_request().unwrap().unwrap();
    assert!(
        quick.visits <= 64,
        "a missing map must not wait for the chart's next 65536-visit run"
    );
    assert_eq!(quick.query["includeOwnership"], true);
    let mut map = vec![0.0; 81];
    map[10] = 0.9;
    let mut partial = reply(&quick.id, 1, 8, true);
    partial["ownership"] = json!(map);
    scheduler.accept(&partial).unwrap();
    let shown = &scheduler.values[&1];
    assert_eq!(
        (shown.visits, shown.score_lead, shown.winrate),
        (16384, 7.25, 0.73)
    );
    assert_eq!(
        shown.ownership, map,
        "a fast map must appear without waiting for the old score depth"
    );
    assert_eq!(
        shown.ownership_depth(),
        8,
        "the overlay must not claim 16384 visits for a fast map"
    );
    assert!(
        review.cached_analysis(&profile()).unwrap()[&1]
            .ownership
            .is_empty(),
        "partial maps stay transient"
    );
    let mut final_reply = reply(&quick.id, 1, 64, false);
    final_reply["ownership"] = json!(map);
    for (target, raw) in scheduler.accept(&final_reply).unwrap() {
        assert_eq!(
            raw.visits, 64,
            "cache input must be the actual completed response, not merged transient state"
        );
        review.store_analysis(&target.key, &raw).unwrap();
    }
    drop(review);
    let mut reopened =
        Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    reopened.import(&source).unwrap();
    let cached = reopened.cached_analysis(&profile()).unwrap();
    assert_eq!(
        (cached[&1].visits, cached[&1].score_lead, cached[&1].winrate),
        (16384, 7.25, 0.73)
    );
    assert_eq!(
        cached[&1].ownership, map,
        "the first eligible group map must be reused offline alongside the deeper chart"
    );
    assert_eq!(cached[&1].ownership_depth(), 64);
    let mut resumed = Scheduler::new(2, reopened.document().unwrap().clone(), profile(), cached);
    assert_eq!(
        resumed.next_request().unwrap().unwrap().visits,
        65536,
        "reopen must skip duplicate quick maps and continue deepening"
    );
}

#[test]
fn ownership_in_a_cheaper_stream_is_visible_while_a_deep_chart_is_kept() {
    let mut scheduler = Scheduler::new(1, doc(), profile(), BTreeMap::new());
    let request = scheduler.next_request().unwrap().unwrap();
    scheduler
        .accept(&reply(&request.id, 1, 4096, true))
        .unwrap();
    let mut fast = reply(&request.id, 1, 1, true);
    fast["ownership"] = json!(vec![0.8; 81]);
    scheduler.accept(&fast).unwrap();
    assert_eq!(scheduler.values[&1].visits, 4096);
    assert_eq!(
        scheduler.values[&1].ownership_depth(),
        1,
        "ownership must have its own depth independent of a newer root evaluation"
    );
    assert_eq!(scheduler.values[&1].ownership.len(), 81);
    fast["rootInfo"]["visits"] = json!(64);
    fast["ownership"] = json!(vec![-0.8; 81]);
    scheduler.accept(&fast).unwrap();
    assert_eq!(scheduler.values[&1].ownership_depth(), 64);
    let before = scheduler.values[&1].clone();
    fast["rootInfo"]["visits"] = json!(8);
    fast["ownership"] = json!(vec![0.0; 81]);
    scheduler.accept(&fast).unwrap();
    assert_eq!(
        scheduler.values[&1], before,
        "a later cheap map must not replace a deeper map"
    );
}
