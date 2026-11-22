#![cfg(feature = "desktop")]
#[path = "support/archive_ui.rs"]
mod archive_ui;
#[path = "support/pandanet/mod.rs"]
mod support;

#[path = "support/category_scores.rs"]
mod category_scores;
use category_scores::{cache_scores, profile};

use archive_ui::{archive_desktop, select_all_time, wait_for};
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
                format!("2026/11/{day:02}"),
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

#[test]
fn overview_splits_filtered_results_by_the_players_recorded_color() {
    let root = tempfile::tempdir().unwrap();
    let mut review = Review::new(&root.path().join("reviews"), &root.path().join("cache")).unwrap();
    let mut ogs = Options::ogs(root.path().join("OGS"));
    ogs.credentials = Arc::new(support::Vault::default());
    seed_archive(
        &ogs,
        &mut review,
        &profile(),
        &[
            (1, "oak", "member", "B+R", &[]),
            (2, "maple", "member", "W+T", &[]),
            (3, "member", "oak", "W+1.5", &[]),
            (4, "member", "elm", "0", &[]),
        ],
    );
    // The original SGF has the authoritative colors even when the index swaps them.
    rusqlite::Connection::open(ogs.root.join("sync.sqlite"))
        .unwrap()
        .execute(
            "UPDATE games SET black='member', white='oak' WHERE id='3'",
            [],
        )
        .unwrap();
    drop(review);
    let mut h = archive_desktop(root.path(), vec![ogs], vec2(1200., 900.));
    h.get_by_label("Games").click();
    select_all_time(&mut h);
    h.get_by_label("Overview").click();
    wait_for(&mut h, "the existing breakdown did not load", |h| {
        h.query_by_label("Unknown time · 2 W / 1 L · 67% win rate")
            .is_some()
    });
    h.get_by_label("As Black · 1 W / 1 L · 50% win rate");
    h.get_by_label("As White · 1 W / 0 L · 100% win rate");
    h.get_by_label("Search synced games").focus();
    h.get_by_label("Search synced games").type_text("maple");
    h.run_steps(3);
    h.get_by_label("As Black · 0 W / 1 L · 0% win rate");
    h.get_by_label("As White · 0 W / 0 L · — win rate");
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
    // A stale archive result must not turn the ordinary SGF win into a classified loss.
    rusqlite::Connection::open(panda.root.join("sync.sqlite"))
        .unwrap()
        .execute("UPDATE games SET result='B+R' WHERE id='3'", [])
        .unwrap();
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
    select_all_time(&mut h);
    let summary = "Comebacks 2/3 wins (67%) · Blunders 1/1 loss (100%) · 1 pending · 1 unavailable";
    wait_for(
        &mut h,
        "expected comeback rate among classified wins and blunder rate among classified losses",
        |h| h.query_by_label(summary).is_some(),
    );

    let first = "Review 2026/11/01: maple vs member";
    let second = "Review 2026/11/02: member vs ash";
    let other = "Review 2026/11/03: member vs cherry";
    let sixth = "Review 2026/11/06: elm vs member";
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
    h.get_by_label("Comebacks 1/1 win (100%) · Blunders 1/1 loss (100%) · 1 unavailable");
    h.get_by_label("Blunders").click();
    h.run_steps(3);
    h.get_by_label(second);
    assert!(h.query_by_label(sixth).is_none());

    h.get_by_label("Search synced games").focus();
    h.get_by_label("Search synced games").type_text("elm");
    h.run_steps(3);
    h.get_by_label("No matching games.");
    h.get_by_label("Comebacks 1/1 win (100%) · Blunders 0/0 losses (—)");
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
    // The cache identity includes the executable, which another Cargo build can replace.
    let executable = root.path().join("test-engine");
    std::fs::copy(env!("CARGO_BIN_EXE_katastro-test-engine"), &executable).unwrap();
    let config = EngineConfig {
        executable,
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
    select_all_time(&mut h);
    wait_for(&mut h, "the cached 20-point swing was missing", |h| {
        h.query_by_label("Comebacks 0/0 wins (—) · Blunders 1/1 loss (100%)")
            .is_some()
    });
    h.get_by_label("Blunders").click();
    h.run_steps(3);
    let row = "Review 2026/11/07: oak vs member";
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
            h.query_by_label("Comebacks 0/0 wins (—) · Blunders 0/1 loss (0%)")
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
            h.query_by_label("Comebacks 0/0 wins (—) · Blunders 0/0 losses (—) · 1 pending")
                .is_some()
        },
    );
}

// An independently chosen threshold must apply to both periods and survive relaunch.
#[test]
fn period_and_thresholds_survive_relaunch_and_change_the_matching_games() {
    use eframe::egui::{Key, Modifiers};
    use egui_kittest::{Harness, kittest::NodeT};
    use katastro::desktop::DesktopApp;

    fn replace_field(h: &mut Harness<'_, DesktopApp>, label: &str, value: &str) {
        h.get_by_label(label).focus();
        h.step();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.step();
        h.get_by_label(label).type_text(value);
        h.step();
    }

    let temp = tempfile::tempdir().unwrap();
    let options = || {
        let mut ogs = Options::ogs(temp.path().join("OGS"));
        ogs.credentials = Arc::new(support::Vault::default());
        ogs
    };
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.save_profile(&profile()).unwrap();
    seed_archive(
        &options(),
        &mut review,
        &profile(),
        &[
            (1, "red oak", "member", "B+R", &[0., -5., -4., 2.]),
            (2, "elm", "member", "W+R", &[0., 35., 3., -4.]),
            (3, "ash", "member", "B+R", &[0., -12., -3., 4.]),
            (4, "oak", "member", "W+R", &[0., 22., 3., -2.]),
            (5, "yew", "member", "B+R", &[0., -30., -3., 1.]),
        ],
    );
    drop(review);
    let launch = || {
        let mut h = archive_desktop(temp.path(), vec![options()], vec2(1200., 900.));
        h.get_by_label("Games").click();
        wait_for(&mut h, "the local library did not open", |h| {
            h.query_by_label("5 synced games").is_some()
        });
        h
    };
    let mut h = launch();
    archive_ui::wait_for_label(&mut h, "Date range");
    h.get_by_label("Date range").click();
    h.run();
    h.get_by_label("Custom…").click();
    h.run();
    replace_field(&mut h, "Start date", "2026-11-03");
    replace_field(&mut h, "End date", "2026-02-30");
    h.get_by_label("Enter valid dates in YYYY-MM-DD format.");
    replace_field(&mut h, "End date", "2026-11-02");
    h.get_by_label("End date must be on or after the start date.");
    assert!(h.get_by_label("Apply dates").accesskit_node().is_disabled());
    replace_field(&mut h, "End date", "2026-11-04");
    h.run();
    h.get_by_label("Apply dates").click();
    h.run();
    let ash = "Review 2026/11/03: ash vs member";
    let oak = "Review 2026/11/04: oak vs member";
    h.get_by_label(ash);
    h.get_by_label(oak);
    assert_eq!(h.query_all_by_label_contains("Review 2026/11/").count(), 2);

    h.get_by_label("Thresholds").click();
    h.run();
    replace_field(&mut h, "Comeback threshold", "0");
    h.get_by_label("Enter a point value greater than zero for each threshold.");
    assert!(
        h.get_by_label("Apply thresholds")
            .accesskit_node()
            .is_disabled()
    );
    replace_field(&mut h, "Comeback threshold", "12");
    replace_field(&mut h, "Blunder threshold", "22.5");
    h.run();
    h.get_by_label("Apply thresholds").click();
    h.run();
    let current = "Comebacks 1/1 win (100%, +100 pp) · Blunders 0/1 loss (0%, -100 pp)";
    let previous =
        "Previous 2026-11-01 – 2026-11-02 · Comebacks 0/1 win (0%) · Blunders 1/1 loss (100%)";
    wait_for(&mut h, "thresholds did not update both periods", |h| {
        h.query_by_label(current).is_some() && h.query_by_label(previous).is_some()
    });
    if let Some(path) = std::env::var_os("KATASTRO_STATISTICS_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    h.get_by_label("Comebacks").click();
    h.run();
    h.get_by_label(ash);
    assert!(h.query_by_label(oak).is_none());
    h.get_by_label("Blunders").click();
    h.run();
    h.get_by_label("No matching games.");
    drop(h);

    let mut reopened = launch();
    wait_for(
        &mut reopened,
        "the date range and thresholds were lost on relaunch",
        |h| h.query_by_label(current).is_some() && h.query_by_label(previous).is_some(),
    );
    assert_eq!(
        reopened
            .query_all_by_label_contains("Review 2026/11/")
            .count(),
        2
    );
    reopened.get_by_label("Compare previous period").click();
    reopened.run();
    assert!(reopened.query_by_label(previous).is_none());
    reopened.get_by_label("Thresholds").click();
    reopened.run();
    reopened.get_by_label("Restore defaults").click();
    reopened.run();
    reopened.get_by_label("Apply thresholds").click();
    reopened.run();
    reopened.get_by_label("Comebacks 0/1 win (0%) · Blunders 1/1 loss (100%)");

    reopened.get_by_label("Overview").click();
    reopened.run();
    replace_field(&mut reopened, "Search synced games", "oak");
    reopened.run();
    reopened.get_by_label("Unknown time · 0 W / 1 L · 0% win rate");
    reopened.get_by_label("Compare previous period").click();
    reopened.run();
    reopened.get_by_label("Unknown time · 0 W / 1 L · 0% win rate · Previous 1 W / 0 L · -100 pp");
    reopened.get_by_label(
        "Blunders · 100% · 1 of 1 classified loss · Previous 0 of 0 classified losses · —",
    );
    reopened.get_by_label("Thresholds").click();
    reopened.run();
    replace_field(&mut reopened, "Blunder threshold", "30");
    reopened.get_by_label("Apply thresholds").click();
    reopened.run();
    reopened.get_by_label(
        "Blunders · 0% · 0 of 1 classified loss · Previous 0 of 0 classified losses · —",
    );
}

#[test]
fn opening_games_while_storage_is_locked_preserves_saved_thresholds() {
    use egui_kittest::kittest::NodeT;
    use katastro::archive::{StatisticsSettings, categories::Thresholds};

    let temp = tempfile::tempdir().unwrap();
    let reviews = temp.path().join("reviews");
    drop(Review::new(&reviews, &temp.path().join("cache")).unwrap());
    let db = rusqlite::Connection::open(reviews).unwrap();
    let saved = StatisticsSettings {
        thresholds: Thresholds {
            comeback: 12.,
            blunder: 34.,
        },
        ..Default::default()
    };
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    db.execute(
        "INSERT INTO settings(key,value) VALUES ('game_statistics',?1)",
        [serde_json::to_string(&saved).unwrap()],
    )
    .unwrap();

    let mut options = Options::ogs(temp.path().join("OGS"));
    options.credentials = Arc::new(support::Vault::default());
    let mut h = archive_desktop(temp.path(), vec![options], vec2(1200., 900.));
    h.get_by_label("Games").click();
    h.run();
    h.get_by_label("0 synced games");
    if let Some(toggle) = h.query_by_label("Compare previous period")
        && !toggle.accesskit_node().is_disabled()
    {
        toggle.click();
        h.run();
    }
    db.execute_batch("COMMIT").unwrap();
    wait_for(
        &mut h,
        "statistics did not load after unlocking storage",
        |h| h.query_by_label("Thresholds").is_some(),
    );
    drop(h); // Flush any command enqueued before startup finished.
    let value: String = db
        .query_row(
            "SELECT value FROM settings WHERE key='game_statistics'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<StatisticsSettings>(&value)
            .unwrap()
            .thresholds,
        saved.thresholds
    );
}

#[path = "support/timezone.rs"]
mod timezone;

#[cfg(unix)]
#[test]
fn scanning_results_keeps_read_games_visible_without_counting_unread_games_as_unknown() {
    use std::{fs, io::Write, process::Command};

    let temp = tempfile::tempdir().unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    let mut ogs = Options::ogs(temp.path().join("OGS"));
    ogs.credentials = Arc::new(support::Vault::default());
    let paths = seed_archive(
        &ogs,
        &mut review,
        &profile(),
        &[
            (1, "member", "ash", "B+R", &[]),
            (2, "member", "elm", "W+R", &[]),
            (3, "member", "oak", "0", &[]),
        ],
    );
    drop(review);
    // The archive scans newest first. Hold the oldest SGF until the two others are displayed.
    let delayed_sgf = fs::read(&paths[0]).unwrap();
    fs::remove_file(&paths[0]).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(&paths[0])
            .status()
            .unwrap()
            .success()
    );
    let mut h = archive_desktop(temp.path(), vec![ogs], vec2(1200., 900.));
    h.get_by_label("Games").click();
    select_all_time(&mut h);
    h.get_by_label("Overview").click();
    wait_for(&mut h, "the completed win did not appear", |h| {
        h.query_by_label("Unknown time · 1 W / 0 L · 100% win rate")
            .is_some()
    });
    h.get_by_label("1 game without a win/loss result");
    h.get_by_label("Reading game results…");
    fs::OpenOptions::new()
        .write(true)
        .open(&paths[0])
        .unwrap()
        .write_all(&delayed_sgf)
        .unwrap();
    wait_for(&mut h, "the completed loss did not appear", |h| {
        h.query_by_label("Unknown time · 1 W / 1 L · 50% win rate")
            .is_some()
    });
    h.get_by_label("1 game without a win/loss result");
    assert!(h.query_by_label("Reading game results…").is_none());
}

// Results without engine analysis must follow local weekday and clock boundaries.
#[test]
fn wins_by_play_time_follow_local_dates_and_the_visible_library_scope() {
    if timezone::rerun_if_needed(
        "wins_by_play_time_follow_local_dates_and_the_visible_library_scope",
        "America/Los_Angeles",
    ) {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    let mut panda = Options::new(temp.path().join("Pandanet"));
    let mut ogs = Options::ogs(temp.path().join("OGS"));
    panda.credentials = Arc::new(support::Vault::default());
    ogs.credentials = panda.credentials.clone();
    let mut seed = |options: &Options, games: &[Game<'_>], dates: &[&str]| {
        seed_archive(options, &mut review, &profile(), games);
        let db = rusqlite::Connection::open(options.root.join("sync.sqlite")).unwrap();
        for (&(id, ..), &date) in games.iter().zip(dates) {
            db.execute(
                "UPDATE games SET date=?1 WHERE id=?2",
                params![date, id.to_string()],
            )
            .unwrap();
        }
    };
    seed(
        &ogs,
        &[
            (1, "ash", "member", "B+R", &[]),
            (2, "member", "birch", "B+R", &[]),
            (3, "member", "ash", "W+T", &[]),
            (4, "maple", "member", "0", &[]),
            (5, "alder", "member", "W+T", &[]),
            (6, "yew", "member", "W+R", &[]),
        ],
        &[
            "2026/08/08 06:59",
            "2026/08/08 07:00",
            "2026-08-08",
            "2026-08-06",
            "",
            "2026/08/01 06:20",
        ],
    );
    seed(
        &panda,
        &[(1, "ash", "member", "W+2.5", &[])],
        &["2026/08/08 15:59"],
    );
    drop(review);
    use katastro::archive::{
        StatisticsSettings,
        period::{Interval, Period},
    };
    use time::macros::date;
    let saved = StatisticsSettings {
        period: Period::Custom(
            Interval::new(date!(2026 - 08 - 03), date!(2026 - 08 - 09)).unwrap(),
        ),
        ..Default::default()
    };
    rusqlite::Connection::open(temp.path().join("reviews"))
        .unwrap()
        .execute(
            "INSERT INTO settings(key,value) VALUES ('game_statistics',?1)",
            [serde_json::to_string(&saved).unwrap()],
        )
        .unwrap();
    let mut h = archive_desktop(temp.path(), vec![panda, ogs], vec2(1200., 900.));
    h.get_by_label("Games").click();
    archive_ui::wait_for_label(&mut h, "Date range");
    h.get_by_label("Blunders").click();
    h.run_steps(3);
    h.get_by_label("Overview").click();
    wait_for(
        &mut h,
        "Friday should include one OGS win and one Pandanet loss",
        |h| {
            h.query_by_label("Friday · 1 W / 1 L · 50% win rate · Previous 0 W / 1 L · +50 pp")
                .is_some()
        },
    );
    if let Some(path) = std::env::var_os("KATASTRO_PLAY_TIMES_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    let footer = h.get_by_label("Close library").rect();
    for label in ["Sunday", "Unknown time"] {
        let row = h.get_by_label_contains(&format!("{label} ·")).rect();
        assert!(
            row.bottom() < footer.top(),
            "{label} is clipped at the default library size"
        );
    }
    let color = h.get_by_label("Record by color").rect();
    let day = h.get_by_label("Day of the week").rect();
    let clock = h.get_by_label("Time of day").rect();
    assert!(color.right() < day.left() && day.right() < clock.left());
    h.set_size(vec2(790., 900.)).run_steps(4);
    let color = h.get_by_label("Record by color").rect();
    let day = h.get_by_label("Day of the week").rect();
    let clock = h.get_by_label("Time of day").rect();
    assert!(day.top() > color.bottom() && clock.left() > color.right());
    assert!(
        h.query_all_by_label("Change")
            .all(|node| node.rect().right() < 790.)
    );
    h.set_size(vec2(1200., 900.)).run_steps(4);
    select_all_time(&mut h);
    h.get_by_label("Friday · 1 W / 2 L · 33% win rate");
    for label in [
        "Saturday · 1 W / 1 L · 50% win rate",
        "Unknown day · 0 W / 1 L · 0% win rate",
        "18:00–23:59 · 1 W / 2 L · 33% win rate",
        "00:00–05:59 · 0 W / 1 L · 0% win rate",
        "12:00–17:59 · 0 W / 0 L · — win rate",
        "Unknown time · 1 W / 1 L · 50% win rate",
        "1 game without a win/loss result",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label("By hour").click();
    h.run_steps(3);
    h.get_by_label("00:00 · 0 W / 1 L · 0% win rate");
    assert!(
        h.query_by_label("00:00–05:59 · 0 W / 1 L · 0% win rate")
            .is_none()
    );
    // Scrolling over the hourly results should move the whole overview while the
    // library's filters and close control remain usable.
    let hour_control = h.get_by_label("By hour").rect();
    let close = h.get_by_label("Close library").rect();
    let dates = h.get_by_label("Date range").rect();
    let table = h.get_by_label("00:00 · 0 W / 1 L · 0% win rate").rect();
    let scroll = |h: &mut egui_kittest::Harness<'_, katastro::desktop::DesktopApp>, at, dy| {
        use eframe::egui::{Event, Modifiers, MouseWheelUnit, TouchPhase};
        h.event(Event::PointerMoved(at));
        h.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0., dy),
            modifiers: Modifiers::NONE,
            phase: TouchPhase::Move,
        });
        h.run_steps(8);
    };
    scroll(&mut h, table.center(), -2_000.);
    let last_hour = h.get_by_label("23:00 · 1 W / 2 L · 33% win rate").rect();
    assert!(last_hour.top() > dates.bottom() && last_hour.bottom() < close.top());
    assert!(
        h.query_by_label("By hour")
            .is_none_or(|node| node.rect().top() < hour_control.top() - 1.),
        "the time controls stayed stuck above a separately scrolling table"
    );
    assert_eq!(h.get_by_label("Date range").rect(), dates);
    assert_eq!(h.get_by_label("Close library").rect(), close);
    scroll(&mut h, last_hour.center(), 2_000.);
    h.get_by_label("OGS games").click();
    h.run_steps(3);
    h.get_by_label("Friday · 1 W / 1 L · 50% win rate");
    h.get_by_label("Search synced games").focus();
    h.get_by_label("Search synced games").type_text("birch");
    h.run_steps(3);
    h.get_by_label("Saturday · 0 W / 1 L · 0% win rate");
    h.get_by_label("00:00 · 0 W / 1 L · 0% win rate");
    h.get_by_label("Game list").click();
    h.run_steps(3);
    h.get_by_label("No matching games.");
    h.get_by_label("All categories").click();
    h.run_steps(3);
    h.get_by_label_contains("member vs birch").click();
    wait_for(&mut h, "the filtered game did not open", |h| {
        h.state().state.document.is_some()
    });
}

// A decided game without scores counts toward the overall record while category
// rates use only games with sufficient analysis for those classifications.
#[test]
fn overview_compares_overall_and_classified_rates_for_the_same_games() {
    use katastro::archive::{
        StatisticsSettings,
        period::{Interval, Period},
    };
    use time::macros::date;

    let temp = tempfile::tempdir().unwrap();
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.save_profile(&profile()).unwrap();
    let mut ogs = Options::ogs(temp.path().join("OGS"));
    ogs.credentials = Arc::new(support::Vault::default());
    seed_archive(
        &ogs,
        &mut review,
        &profile(),
        &[
            (1, "maple", "member", "W+R", &[0., 3., -2., -1.]),
            (3, "maple", "member", "B+R", &[0., -25., 3., 2.]),
            (4, "maple", "member", "W+R", &[0., 26., -1., -2.]),
            (5, "member", "elm", "B+R", &[0., -3., 1., 2.]),
            (6, "elm", "member", "B+R", &[]),
            (7, "elm", "member", "Draw", &[]),
        ],
    );
    drop(review);
    let saved = StatisticsSettings {
        period: Period::Custom(
            Interval::new(date!(2026 - 11 - 03), date!(2026 - 11 - 07)).unwrap(),
        ),
        ..Default::default()
    };
    rusqlite::Connection::open(temp.path().join("reviews"))
        .unwrap()
        .execute(
            "INSERT INTO settings(key,value) VALUES ('game_statistics',?1)",
            [serde_json::to_string(&saved).unwrap()],
        )
        .unwrap();
    let mut h = archive_desktop(temp.path(), vec![ogs], vec2(1200., 900.));
    h.get_by_label("Games").click();
    archive_ui::wait_for_label(&mut h, "Date range");
    h.get_by_label("Overview").click();
    wait_for(
        &mut h,
        "the overall record omitted an unanalyzed win or included a draw",
        |h| {
            h.query_by_label("Win rate · 50% · 2 W / 2 L · Previous 0 W / 1 L · +50 pp")
                .is_some()
        },
    );
    h.get_by_label(
        "Comebacks · 100% · 1 of 1 classified win · Previous 0 of 0 classified wins · —",
    );
    h.get_by_label(
        "Blunders · 50% · 1 of 2 classified losses · Previous 0 of 1 classified loss · +50 pp",
    );
    h.get_by_label("1 game without a win/loss result");
    h.get_by_label("Search synced games").focus();
    h.get_by_label("Search synced games").type_text("elm");
    h.run_steps(3);
    h.get_by_label("Win rate · 50% · 1 W / 1 L · Previous 0 W / 0 L · —");
    h.get_by_label("Comebacks · — · 0 of 0 classified wins · Previous 0 of 0 classified wins · —");
    h.get_by_label(
        "Blunders · 0% · 0 of 1 classified loss · Previous 0 of 0 classified losses · —",
    );
}
