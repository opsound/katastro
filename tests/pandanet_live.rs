use katastro::Review;
#[test]
#[ignore = "Requires KATASTRO_PANDANET_SGF pointing to a private authenticated Pandanet download"]
fn live_pandanet_download_imports_and_replays() {
    let source = std::env::var_os("KATASTRO_PANDANET_SGF").expect("set KATASTRO_PANDANET_SGF");
    let tmp = tempfile::tempdir().unwrap();
    let mut review = Review::new(
        &tmp.path().join("review.sqlite"),
        &tmp.path().join("cache.sqlite"),
    )
    .unwrap();
    review.import(std::path::Path::new(&source)).unwrap();
    let doc = review.document().unwrap();
    assert!(doc.mainline.len() > 1);
    doc.board(*doc.mainline.last().unwrap()).unwrap();
}

#[path = "support/pandanet/mod.rs"]
mod support;
#[test]
#[ignore = "Live network benchmark: requires KATASTRO_PANDANET_ARCHIVE_HTML with six authenticated SGF links and KATASTRO_PANDANET_USERNAME; downloads that sample twice"]
fn live_download_batch_compares_serial_pacing_with_two_parallel_transfers() {
    use katastro::pandanet::{Cancellation, Command};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let page = std::fs::read_to_string(
        std::env::var_os("KATASTRO_PANDANET_ARCHIVE_HTML").expect("private HTML path required"),
    )
    .unwrap();
    let username = std::env::var("KATASTRO_PANDANET_USERNAME").expect("username required");
    // The local server supplies an already authenticated list; downloads use the
    // production HTTPS client against the real returned Pandanet URLs.
    let fixture = support::Fixture::new(move |_| (200, page.clone()));
    for (name, parallel, spacing) in [
        ("serial_one_second", 1, Duration::from_secs(1)),
        ("parallel_two", 2, Duration::ZERO),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = fixture.service(tmp.path(), Arc::new(support::Vault::default()));
        s.set_parallel_downloads(parallel);
        s.set_request_interval(spacing);
        let started = Instant::now();
        let result = s.execute(
            Command::Connect {
                username: username.clone(),
                password: "local-fixture-only".into(),
                remember: false,
            },
            100,
            &Cancellation::default(),
            &mut |_| {},
        );
        let elapsed = started.elapsed().as_secs_f64();
        let state = s.snapshot();
        let bytes: usize = state
            .games
            .iter()
            .map(|g| std::fs::metadata(&g.path).unwrap().len() as usize)
            .sum();
        let moves: Vec<_> = state
            .games
            .iter()
            .map(|g| {
                katastro::Document::parse(&std::fs::read(&g.path).unwrap())
                    .unwrap()
                    .mainline
                    .len()
                    - 1
            })
            .collect();
        println!(
            "{{\"mode\":\"{name}\",\"seconds\":{elapsed:.3},\"games\":{},\"bytes\":{bytes},\"moves\":{moves:?}}}",
            state.games.len()
        );
        result.unwrap();
        assert_eq!(state.games.len(), 6);
    }
}
