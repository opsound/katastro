#![allow(dead_code)]
use katastro::{EngineProfile, training::Trainer};
use serde_json::{Value, json};
use std::path::Path;
pub fn profile() -> EngineProfile {
    EngineProfile {
        model_sha256: "test-model".into(),
        engine_sha256: "test-engine".into(),
        settings_digest: "test-semantics".into(),
    }
}
pub fn fixture(root: &Path, white: bool) -> std::path::PathBuf {
    let path = root.join("game.sgf");
    let prefix = if white { ";B[ii]" } else { "" };
    let moves: String = (0..14)
        .map(|i| {
            format!(
                ";{}[{}{}]",
                if (i % 2 == 0) != white { "B" } else { "W" },
                (b'a' + i % 9) as char,
                (b'a' + i / 9) as char
            )
        })
        .collect();
    std::fs::write(
        &path,
        format!(
            "(;SZ[9]KM[6.5]PB[{}]PW[{}]C[SECRET SOLUTION]{prefix}{moves})",
            if white { "opponent" } else { "me" },
            if white { "me" } else { "opponent" }
        ),
    )
    .unwrap();
    path
}
pub fn trainer(root: &Path) -> Trainer {
    Trainer::new(
        &root.join("reviews"),
        &root.join("cache"),
        &root.join("Training"),
        profile(),
    )
    .unwrap()
}
// Fixed outcomes are in Black's perspective. A divergent attempt can recover the lead.
pub fn reply(q: &Value, white: bool, good: bool) -> Value {
    let moves = q["moves"].as_array().unwrap();
    let key_turn = usize::from(white);
    let divergent = moves
        .iter()
        .skip(key_turn)
        .any(|m| m[1].as_str().unwrap().ends_with('5'));
    let before = moves.len() <= key_turn;
    let (wr, score) = if before || (divergent && good) {
        (0.8, 8.0)
    } else {
        (0.4, -2.0)
    };
    let mut r = json!({"id":q["id"],"turnNumber":moves.len(),"isDuringSearch":false,"rootInfo":{"visits":q["maxVisits"],"winrate":if white {1.0-wr} else {wr},"scoreLead":if white {-score} else {score}}});
    r["moveInfos"] = json!([{"move":format!("{}5", b"ABCDEFGHJ"[(moves.len().saturating_sub(key_turn)/2)%9] as char),"visits":q["maxVisits"],"winrate":0.5,"scoreLead":8.0}]);
    if q["includePolicy"] == true {
        let mut policy = vec![-1.0; 82];
        policy[81] = 1.0;
        r["humanPolicy"] = json!(policy);
    }
    if q["overrideSettings"]["analysisPVLen"].as_u64().unwrap_or(1) > 1 {
        r["moveInfos"] = json!([
    {"move":"D5","visits":240,"winrate":if white {0.2} else {0.8},"scoreLead":if white {-8.0} else {8.0},"pv":["D5","E5","F5"]},
    {"move":"E5","visits":200,"winrate":if white {0.21} else {0.79},"scoreLead":if white {-7.5} else {7.5},"pv":["E5","D5","C5"]},
    {"move":"A1","visits":32,"winrate":0.5,"scoreLead":0.0,"pv":["A1","B1"]}]);
    }
    r
}
pub fn settle(t: &mut Trainer, white: bool, good: bool) -> Vec<Value> {
    settle_with(t, |q| reply(q, white, good))
}
pub fn settle_with(t: &mut Trainer, mut respond: impl FnMut(&Value) -> Value) -> Vec<Value> {
    let mut requests = vec![];
    for _ in 0..300 {
        let Some(q) = t.next_request().unwrap() else {
            return requests;
        };
        t.accept(&respond(&q), 0.5).unwrap();
        requests.push(q);
    }
    panic!("unbounded exercise work")
}

pub fn searched_moves(node: usize, visits: u64) -> Vec<katastro::SuggestedMove> {
    vec![katastro::SuggestedMove {
        point: Some(katastro::Point::new((node / 2) % 9, 4)),
        visits,
        winrate: Some(0.5),
        score_lead: Some(8.0),
    }]
}
