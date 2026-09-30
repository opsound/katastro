use katastro::Document;
#[test]
#[ignore = "Requires KATASTRO_SGF_DIR pointing to a private local SGF corpus"]
fn real_sgf_corpus_imports_and_replays_to_the_end() {
    let directory = std::env::var("KATASTRO_SGF_DIR").expect("KATASTRO_SGF_DIR is required");
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sgf"))
        })
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let start = std::time::Instant::now();
    let mut failures = Vec::new();
    let mut positions = 0;
    for path in &files {
        match Document::parse(&std::fs::read(path).unwrap()) {
            Ok(doc) => {
                let last = *doc.mainline.last().unwrap();
                doc.board(last).unwrap();
                positions += doc.mainline.len();
            }
            Err(error) => failures.push(format!(
                "{}: {error}",
                path.file_name().unwrap().to_string_lossy()
            )),
        }
    }
    println!(
        "{} games, {positions} played positions, {:.3}s",
        files.len(),
        start.elapsed().as_secs_f64()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
