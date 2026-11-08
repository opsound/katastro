use katastro::{Document, Review};
use std::{hint::black_box, path::PathBuf, time::Instant};

// Explicit local benchmark, never a timing assertion in the routine test suite.
#[test]
#[ignore = "Requires KATASTRO_SGF_DIR; optional KATASTRO_PERF_RECORD or KATASTRO_PERF_BASELINE"]
fn import_and_position_preparation_measurements() {
    let directory = PathBuf::from(std::env::var_os("KATASTRO_SGF_DIR").expect("KATASTRO_SGF_DIR"));
    let mut games: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sgf")))
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect();
    games.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!games.is_empty());
    let mut imports = Vec::new();
    let mut preparation = Vec::new();
    let mut positions = 0;
    for _ in 0..3 {
        let start = Instant::now();
        let docs: Vec<_> = games
            .iter()
            .map(|(_, bytes)| Document::parse(bytes).unwrap())
            .collect();
        imports.push(start.elapsed().as_secs_f64());
        positions = docs.iter().map(|d| d.nodes.len()).sum::<usize>();
        let start = Instant::now();
        for doc in &docs {
            for id in 0..doc.nodes.len() {
                let _ = black_box(doc.position(id));
            }
        }
        preparation.push(start.elapsed().as_secs_f64());
    }
    // Also exercise public import commands with SQLite and final-board selection.
    let temp = tempfile::TempDir::new().unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    for (path, _) in &games {
        review.import(path).unwrap();
        let last = *review.document().unwrap().mainline.last().unwrap();
        review.select(last).unwrap();
        assert!(review.document().unwrap().board(last).is_ok());
    }
    imports.sort_by(f64::total_cmp);
    preparation.sort_by(f64::total_cmp);
    let measured = serde_json::json!({
        "games":games.len(), "positions":positions,
        "corpus":katastro::digest(&serde_json::to_vec(&games.iter().map(|(_, b)| katastro::digest(b)).collect::<Vec<_>>()).unwrap()),
        "import_seconds":imports[1], "position_seconds":preparation[1],
    });
    println!("{measured}");
    if let Some(path) = std::env::var_os("KATASTRO_PERF_RECORD") {
        std::fs::write(path, measured.to_string()).unwrap();
    }
    if let Some(path) = std::env::var_os("KATASTRO_PERF_BASELINE") {
        let baseline: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(measured["corpus"], baseline["corpus"]);
        for metric in ["import_seconds", "position_seconds"] {
            assert!(
                measured[metric].as_f64().unwrap() < baseline[metric].as_f64().unwrap() * 0.75,
                "{metric} must improve by at least 25%: baseline={}, measured={}",
                baseline[metric],
                measured[metric]
            );
        }
    }
}
