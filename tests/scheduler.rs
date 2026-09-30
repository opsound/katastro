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
        256,
        "reopen must skip duplicate coverage requests"
    );
}
#[test]
fn selecting_a_new_variation_interrupts_deep_work_and_ignores_stale_replies() {
    let mut cached = BTreeMap::new();
    for node in 0..4 {
        cached.insert(
            node,
            Analysis {
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
