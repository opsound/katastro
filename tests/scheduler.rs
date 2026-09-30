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
    let deep = scheduler.next_request().unwrap().unwrap();
    assert_eq!(deep.visits, 8);
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
        8,
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
