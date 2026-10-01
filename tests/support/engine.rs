use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
fn emit(query: &Value) {
    if query["maxVisits"].as_u64().is_some_and(|v| v >= 16384) {
        return;
    }
    let turns = query["analyzeTurns"]
        .as_array()
        .cloned()
        .unwrap_or(vec![json!(0)]);
    for turn in turns.iter().rev() {
        let root = json!({"visits":query["maxVisits"].as_u64().unwrap_or(1),"winrate":0.5,"scoreLead":turn.as_u64().unwrap_or(0) as f64});
        let mut result =
            json!({"id":query["id"],"turnNumber":turn,"isDuringSearch":false,"rootInfo":root});
        if query["_testMoves"] == true {
            result["moveInfos"] = json!([
                {"move":"D6","visits":32,"winrate":0.6,"scoreLead":1.0},
                {"move":"E5","visits":32,"winrate":0.5,"scoreLead":0.5}
            ]);
        }
        if query["_variationMoves"] == true {
            result["moveInfos"] = json!([
                {"move":"F5","visits":32,"winrate":0.6,"scoreLead":root["scoreLead"].as_f64().unwrap() + 1.5}
            ]);
        }
        if query["includePolicy"] == true && query["_variationFixture"] == true {
            result["policy"] = json!(vec![1.0 / 82.0; 82]);
        }
        if query["includeOwnership"] == true {
            let width = query["boardXSize"].as_u64().unwrap_or(9) as usize;
            let height = query["boardYSize"].as_u64().unwrap_or(9) as usize;
            let mut ownership = vec![0.0; width * height];
            ownership[0] = 0.9;
            ownership[width * height - 1] = -0.9;
            result["ownership"] = json!(ownership);
        }
        if query.get("reportDuringSearchEvery").is_some() {
            result["isDuringSearch"] = json!(true);
            println!("{result}");
        }
        result["isDuringSearch"] = json!(false);
        println!("{result}");
    }
    io::stdout().flush().unwrap();
}
fn main() {
    eprintln!("test engine ready");
    let args: Vec<_> = std::env::args_os().collect();
    let model = args
        .windows(2)
        .find(|pair| pair[0] == "-model")
        .map(|pair| std::fs::read(&pair[1]).unwrap());
    let invalid_evaluation = model.as_deref() == Some(b"bad-evaluation");
    let hold_final = model.as_deref() == Some(b"hold-final");
    let variation_fixture = model.as_deref() == Some(b"variation-hints");
    let mut first_variation = None;
    let mut gate_path = model
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|text| text.strip_prefix("gate-deep:"))
        .map(std::path::PathBuf::from);
    let visual_fixture = gate_path.is_some();
    let mut held = Vec::new();
    for line in io::stdin().lock().lines() {
        let mut query: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if visual_fixture {
            query["_testMoves"] = json!(true);
            if query["maxVisits"]
                .as_u64()
                .is_some_and(|visits| visits >= 256)
                && let Some(path) = gate_path.take()
            {
                // A named pipe lets the UI test release refinement after its click.
                let mut gate = std::fs::File::open(path).unwrap();
                gate.read_exact(&mut [0]).unwrap();
            }
        }
        match query["action"].as_str() {
            Some("hold") => {
                held.push(query["query"].clone());
                println!("{}", json!({"id":query["id"],"action":"held"}));
            }
            Some("release") => {
                for q in held.drain(..) {
                    emit(&q);
                }
            }
            Some("terminate") | Some("terminate_all") => {
                println!("{query}");
                if let Some((id, turn)) = &first_variation
                    && query["terminateId"] == *id
                {
                    // Late completion of canceled quick work must not suppress new hints.
                    println!(
                        "{}",
                        json!({"id":id,"turnNumber":turn,"isDuringSearch":false,"noResults":true})
                    );
                }
            }
            Some("crash") => std::process::exit(7),
            Some("malformed") => println!("invalid engine output"),
            Some("query_version") => println!(
                "{}",
                json!({"id":query["id"],"version":"fake-test-only","action":"query_version"})
            ),
            _ if invalid_evaluation => println!(
                "{}",
                json!({"id":query["id"],"turnNumber":0,"isDuringSearch":false,"rootInfo":{"visits":0,"winrate":0.5,"scoreLead":0.0}})
            ),
            _ if hold_final && query["maxVisits"].as_u64().is_some_and(|v| v >= 64) => println!(
                "{}",
                json!({"id":query["id"],"turnNumber":0,"isDuringSearch":true,"rootInfo":{"visits":64,"winrate":0.6,"scoreLead":2.5}})
            ),
            _ if variation_fixture && query["priority"] == 0 => {
                // Keep original-game coverage pending while real UI input creates branches.
                eprintln!("fixture: coverage held");
            }
            _ if variation_fixture && query["priority"] == 100 && first_variation.is_none() => {
                first_variation = Some((query["id"].clone(), query["analyzeTurns"][0].clone()));
                eprintln!("fixture: first variation held");
            }
            _ if variation_fixture => {
                query["_variationFixture"] = json!(true);
                query["_variationMoves"] = json!(query["maxVisits"].as_u64().unwrap() >= 64);
                emit(&query);
            }
            _ => emit(&query),
        }
        io::stdout().flush().unwrap();
    }
}
