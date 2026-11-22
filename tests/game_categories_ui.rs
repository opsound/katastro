#![cfg(feature = "desktop")]
#[path = "support/archive_ui.rs"]
mod archive_ui;
#[path = "support/pandanet/mod.rs"]
mod support;

#[path = "support/category_scores.rs"]
mod category_scores;
use category_scores::{cache_scores, profile};

use archive_ui::{archive_desktop, wait_for};
use eframe::egui::vec2;
use egui_kittest::kittest::Queryable;
use katastro::{
    EngineProfile, Review,
    archive::{Options, Service},
};
use rusqlite::params;
use std::{path::PathBuf, sync::Arc};

// Day, White, Black, result and Black's cached score after each main-line position.
type Game<'a> = (u8, &'a str, &'a str, &'a str, &'a [f64]);

fn seed_archive(
    option: &Options,
    review: &mut Review,
    engine: &EngineProfile,
    games: &[Game<'_>],
) -> Vec<PathBuf> {
    drop(
        Service::open_for(
            &option.root,
            &option.endpoint,
            option.credentials.clone(),
            option.server,
        )
        .unwrap(),
    );
    let db = rusqlite::Connection::open(option.root.join("sync.sqlite")).unwrap();
    db.execute(
        "INSERT INTO config(singleton,username,remember,automatic) VALUES(1,'member',0,0)",
        [],
    )
    .unwrap();
    let mut paths = Vec::new();
    for &(day, white, black, result, scores) in games {
        let filename = format!("{day}.sgf");
        let path = option.root.join(&filename);
        // Distinct komi keeps independently seeded positions in separate cache entries.
        std::fs::write(
            &path,
            format!(
                "(;FF[4]GM[1]SZ[19]KM[{day}.5]PW[{white}]PB[{black}]RE[{result}];B[dd];W[pp];B[pd])"
            ),
        )
        .unwrap();
        db.execute(
            "INSERT INTO games(username,id,date,white,black,result,filename,sha) VALUES('member',?1,?2,?3,?4,?5,?6,'')",
            params![
                day.to_string(),
                format!("2026/11/{day:02} 12:00"),
                white,
                black,
                result,
                filename
            ],
        )
        .unwrap();
        cache_scores(review, &path, engine, scores);
        paths.push(path.canonicalize().unwrap());
    }
    paths
}

// A player can see how often a classified game changed hands, narrow the library
// by category, server and search, then open the matching original SGF.
#[test]
fn games_counts_and_filters_follow_the_players_outcomes() {
    let root = tempfile::tempdir().unwrap();
    let mut review = Review::new(&root.path().join("reviews"), &root.path().join("cache")).unwrap();
    review.save_profile(&profile()).unwrap();
    let mut panda = Options::new(root.path().join("Pandanet"));
    let mut ogs = Options::ogs(root.path().join("OGS"));
    panda.credentials = Arc::new(support::Vault::default());
    ogs.credentials = panda.credentials.clone();
    seed_archive(
        &panda,
        &mut review,
        &profile(),
        &[
            (1, "maple", "member", "B+R", &[3., -22., 0., 4.]),
            (3, "member", "cherry", "W+2.5", &[0., -5., -4., -2.5]),
            (4, "fern", "member", "W+R", &[0.]),
        ],
    );
    let ogs_paths = seed_archive(
        &ogs,
        &mut review,
        &profile(),
        &[
            (2, "member", "ash", "B+R", &[1., -25., 0., 8.]),
            (5, "member", "cedar", "Draw", &[0., 1., 2., 0.]),
            (6, "elm", "member", "B+R", &[2., -21., 1., 4.]),
        ],
    );
    drop(review);

    let mut h = archive_desktop(root.path(), vec![panda, ogs], vec2(1200., 900.));
    h.get_by_label("Games").click();
    wait_for(&mut h, "the synced games did not load", |h| {
        h.query_by_label("6 synced games").is_some()
    });
    let summary = "4 classified · 2 comebacks (50%) · 1 blunder (25%) · 1 pending · 1 unavailable";
    wait_for(
        &mut h,
        "cached game categories never appeared in Games",
        |h| h.query_by_label(summary).is_some(),
    );

    let first = "Review 2026/11/01 12:00: maple vs member";
    let second = "Review 2026/11/02 12:00: member vs ash";
    let other = "Review 2026/11/03 12:00: member vs cherry";
    let sixth = "Review 2026/11/06 12:00: elm vs member";
    h.get_by_label("Comebacks").click();
    h.run_steps(3);
    h.get_by_label(first);
    h.get_by_label(sixth);
    assert!(h.query_by_label(second).is_none() && h.query_by_label(other).is_none());
    h.get_by_label(summary);

    h.get_by_label("OGS games").click();
    h.run_steps(3);
    h.get_by_label(sixth);
    assert!(h.query_by_label(first).is_none());
    h.get_by_label("2 classified · 1 comeback (50%) · 1 blunder (50%) · 1 unavailable");
    h.get_by_label("Blunders").click();
    h.run_steps(3);
    h.get_by_label(second);
    assert!(h.query_by_label(sixth).is_none());

    h.get_by_label("Search synced games").focus();
    h.get_by_label("Search synced games").type_text("elm");
    h.run_steps(3);
    h.get_by_label("No matching games.");
    h.get_by_label("1 classified · 1 comeback (100%) · 0 blunders (0%)");
    h.get_by_label("All categories").click();
    h.run_steps(3);
    assert!(h.query_by_label(second).is_none());
    h.get_by_label(sixth).click();
    wait_for(&mut h, "the matching original did not open", |h| {
        h.state().state.source.as_deref() == Some(ogs_paths[2].as_path())
    });
}

// Starting a deeper review while the library is open can overturn an old crossing.
#[test]
fn a_deeper_review_updates_the_active_blunders_filter() {
    use katastro::engine::{AnalysisSettings, Engine, EngineConfig};
    use katastro::worker::Command;

    let root = tempfile::tempdir().unwrap();
    let model = root.path().join("model");
    std::fs::write(&model, b"synthetic").unwrap();
    let config = EngineConfig {
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model: model.clone(),
        analysis: AnalysisSettings {
            max_visits: Some(256),
            ..Default::default()
        },
    };
    let engine = Engine::profile(&config).unwrap();
    let mut review = Review::new(&root.path().join("reviews"), &root.path().join("cache")).unwrap();
    let mut ogs = Options::ogs(root.path().join("OGS"));
    ogs.credentials = Arc::new(support::Vault::default());
    let paths = seed_archive(
        &ogs,
        &mut review,
        &engine,
        &[(7, "oak", "member", "W+R", &[3., 23., 5., -2.])],
    );
    drop(review);

    let mut h = archive_desktop(root.path(), vec![ogs], vec2(1200., 900.));
    h.state()
        .client
        .send(Command::Open(paths[0].clone()))
        .unwrap();
    h.state().client.send(Command::Configure(config)).unwrap();
    wait_for(&mut h, "the configured review did not open", |h| {
        h.state().state.source.as_ref() == Some(&paths[0])
            && h.state().state.config.analysis.max_visits == Some(256)
    });
    h.get_by_label("Games").click();
    wait_for(&mut h, "the cached 20-point swing was missing", |h| {
        h.query_by_label("1 classified · 0 comebacks (0%) · 1 blunder (100%)")
            .is_some()
    });
    h.get_by_label("Blunders").click();
    h.run_steps(3);
    let row = "Review 2026/11/07 12:00: oak vs member";
    h.get_by_label(row);

    h.state().client.send(Command::Start).unwrap();
    wait_for(&mut h, "deeper foreground analysis did not finish", |h| {
        let values = &h.state().state.values;
        values.len() == 4 && values.values().all(|value| value.visits == 256)
    });
    wait_for(
        &mut h,
        "the new 0-to-3 point scores did not remove the classified blunder",
        |h| {
            h.query_by_label("1 classified · 0 comebacks (0%) · 0 blunders (0%)")
                .is_some()
        },
    );
    h.get_by_label("No matching games.");
    assert!(h.query_by_label(row).is_none());
    h.get_by_label("All categories").click();
    h.run_steps(3);
    h.get_by_label(row);

    std::fs::write(model, b"replacement model").unwrap();
    wait_for(
        &mut h,
        "the replacement model inherited an earlier model's scores",
        |h| {
            h.query_by_label("0 classified · 0 comebacks · 0 blunders · 1 pending")
                .is_some()
        },
    );
}
