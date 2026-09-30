use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
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
    let mut held = Vec::new();
    for line in io::stdin().lock().lines() {
        let query: Value = serde_json::from_str(&line.unwrap()).unwrap();
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
            _ => emit(&query),
        }
        io::stdout().flush().unwrap();
    }
}
