#![cfg(feature = "desktop")]
#[path = "support/archive_ui.rs"]
pub mod archive_ui;
#[path = "support/pandanet/mod.rs"]
mod support;
#[path = "support/timezone.rs"]
mod timezone;
#[path = "support/training.rs"]
mod training_support;
use archive_ui::{select_all_time, wait_for, wait_for_label};
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{
    desktop::DesktopApp,
    engine::EngineConfig,
    pandanet::{Cancellation, Command, Options, Snapshot, SystemClock},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
fn desktop(
    root: &Path,
    fixture: &support::Fixture,
    vault: Arc<support::Vault>,
    source: Option<PathBuf>,
    height: f32,
) -> Harness<'static, DesktopApp> {
    Harness::builder()
        .with_size(eframe::egui::vec2(1200., height))
        .build_eframe(|cc| {
            DesktopApp::with_sync(
                &cc.egui_ctx,
                root.join("reviews.sqlite"),
                root.join("cache.sqlite"),
                EngineConfig {
                    analysis: Default::default(),
                    executable: "missing".into(),
                    model: "missing".into(),
                },
                source,
                Options {
                    server: Default::default(),
                    root: root.join("Pandanet"),
                    endpoint: fixture.endpoint.clone(),
                    request_interval: Duration::ZERO,
                    credentials: vault,
                    clock: Arc::new(SystemClock),
                },
            )
        })
}

fn sync(root: &Path, fixture: &support::Fixture, vault: Arc<support::Vault>) -> Snapshot {
    let mut service = fixture.service(&root.join("Pandanet"), vault);
    for (now, command) in [
        (
            100,
            Command::Connect {
                username: "example".into(),
                password: "test-secret".into(),
                remember: true,
            },
        ),
        (101, Command::SetAutomatic(false)),
    ] {
        service
            .execute(command, now, &Cancellation::default(), &mut |_| {})
            .unwrap();
    }
    service.snapshot()
}

fn review_row(game: &katastro::archive::Game) -> String {
    format!("Review {}: {} vs {}", game.date, game.white, game.black)
}

fn wait(h: &mut Harness<'_, DesktopApp>, done: impl Fn(&DesktopApp) -> bool) {
    wait_for(h, "desktop did not finish the requested operation", |h| {
        done(h.state())
    });
}

// A game played today can already have tomorrow's date in Japan. It must
// appear as soon as sync finishes with Last 30 days selected, and after relaunch.
#[test]
fn last_30_days_shows_a_new_pandanet_game_across_midnight() {
    if timezone::rerun_if_needed(
        "last_30_days_shows_a_new_pandanet_game_across_midnight",
        "Pacific/Honolulu",
    ) {
        return;
    }
    let today = time::OffsetDateTime::now_local().unwrap().date();
    let local_date = today.to_string().replace('-', "/");
    let server_date = format!("{} 00:15", today.next_day().unwrap()).replace('-', "/");
    let html = support::page(1, 1, &[1]).replace("2026/11/01 12:00", &server_date);
    let fixture = support::Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            (200, support::sgf(1))
        } else {
            (200, html.clone())
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let mut h = desktop(temp.path(), &fixture, vault.clone(), None, 900.);
    h.get_by_label("Pandanet").click();
    wait_for_label(&mut h, "Date range");
    h.get_by_label("Date range").click();
    wait_for_label(&mut h, "Last 30 days");
    h.get_by_label("Last 30 days").click();
    h.run_steps(3);
    for (label, value) in [
        ("Pandanet username", "example"),
        ("Pandanet password", "test-secret"),
    ] {
        h.get_by_label(label).focus();
        h.get_by_label(label).type_text(value);
        h.run_steps(3);
    }
    h.get_by_label("Connect and sync").click();
    wait_for_label(&mut h, "1 synced games");
    let row = format!("Review {local_date} 05:15: example 6k+ vs opponent 5k+");
    h.get_by_label(&row);
    drop(h);

    // Reopen an archive in the format earlier releases saved, without the server.
    drop(fixture);
    let db = rusqlite::Connection::open(temp.path().join("Pandanet/sync.sqlite")).unwrap();
    db.execute("UPDATE games SET date=?1", [server_date])
        .unwrap();
    let offline = support::Fixture::new(|_| (503, String::new()));
    let mut h = desktop(temp.path(), &offline, vault, None, 900.);
    h.get_by_label("Games").click();
    wait_for_label(&mut h, "Date range");
    wait_for_label(&mut h, &row);
    h.run_steps(3);
    h.get_by_label(&row).click();
    wait(&mut h, |app| app.state.document.is_some());
    assert_eq!(h.state().state.document.as_ref().unwrap().mainline.len(), 4);
    assert!(offline.requests.lock().unwrap().is_empty());
}
// Benefit: a synced library actually starts preparing in an empty window, and
// the user can pause it or enter training without competing background work.
#[test]
fn empty_desktop_prepares_the_synced_library_and_yields_to_training() {
    use std::io::{BufRead, BufReader, Write};
    fn next(engine: &mut BufReader<std::net::TcpStream>) -> serde_json::Value {
        let mut line = String::new();
        engine.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, support::sgf(1))
        } else {
            (200, support::page(1, 1, &[1]))
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let vault = Arc::new(support::Vault::default());
    let library = sync(root, &fixture, vault.clone());
    let source = training_support::fixture(root, false);
    std::fs::write(
        &library.games[0].path,
        std::fs::read_to_string(source)
            .unwrap()
            .replace("PB[me]", "PB[example]"),
    )
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let model = root.join("model");
    std::fs::write(
        &model,
        format!("controlled:{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1200., 820.))
        .build_eframe(|cc| {
            DesktopApp::with_sync(
                &cc.egui_ctx,
                root.join("reviews.sqlite"),
                root.join("cache.sqlite"),
                EngineConfig {
                    analysis: Default::default(),
                    executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
                    model,
                },
                None,
                Options {
                    server: Default::default(),
                    root: root.join("Pandanet"),
                    endpoint: fixture.endpoint.clone(),
                    request_interval: Duration::ZERO,
                    credentials: vault,
                    clock: Arc::new(SystemClock),
                },
            )
        });
    wait(&mut h, |app| app.state.preparing_training);
    let (engine, _) = listener.accept().unwrap();
    engine
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut engine = BufReader::new(engine);
    let held = (0..8).map(|_| next(&mut engine)).collect::<Vec<_>>();
    h.get_by_label("Pause preparation").click();
    wait(&mut h, |app| !app.state.preparing_training);
    assert_eq!(next(&mut engine)["action"], "terminate_all");
    for query in &held {
        writeln!(
            engine.get_mut(),
            "{}",
            training_support::reply(query, false, true)
        )
        .unwrap();
    }
    h.run_steps(3);
    h.get_by_label("Prepare training").click();
    wait(&mut h, |app| app.state.preparing_training);
    let restarted = next(&mut engine);
    assert_ne!(restarted["id"], held[0]["id"]);
    assert_eq!(
        restarted["moves"].as_array().unwrap().len(),
        0,
        "canceled replies must not populate the cache or advance preparation"
    );
    h.get_by_label("Training").click();
    wait(&mut h, |app| !app.state.preparing_training);
    h.run_steps(3);
    assert!(h.query_by_label("Start training").is_some());
}
#[test]
fn desktop_signs_in_remembers_downloads_and_opens_a_game_without_disturbing_a_review() {
    let (entered, arrived) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let fixture = support::Fixture::new(move |r| {
        if r.url.contains("mode=sgf") {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(10)).unwrap();
            (200, support::sgf(1))
        } else {
            (200, support::page(1, 1, &[1]))
        }
    });
    struct Release(std::sync::mpsc::Sender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let release = Release(release);
    let temp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let source = temp.path().join("original.sgf");
    std::fs::write(&source, b"(;SZ[9];B[cc];W[gg])").unwrap();
    let mut h = desktop(temp.path(), &fixture, vault.clone(), Some(source), 860.);
    wait(&mut h, |app| app.state.document.is_some());
    h.get_by_label("Pandanet").click();
    h.run_steps(3);
    h.get_by_label("Pandanet username").focus();
    h.get_by_label("Pandanet username").type_text("example");
    h.get_by_label("Pandanet password").focus();
    h.get_by_label("Pandanet password").type_text("test-secret");
    h.run_steps(3);
    h.get_by_label("Remember login in Keychain").click();
    h.run_steps(3);
    h.get_by_label("Connect and sync").click();
    h.run_steps(3);
    arrived
        .recv_timeout(Duration::from_secs(3))
        .expect("desktop must start the download");
    // The server holds a real request pending while the user navigates the board.
    h.get_by_label("Close library").click();
    h.run_steps(3);
    h.get_by_label("Next move").click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().selected == 1
    });
    h.get_by_label("Play D6").click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().nodes.len() == 4
    });
    release.0.send(()).unwrap();
    h.get_by_label("Pandanet").click();
    select_all_time(&mut h);
    let players = "example 6k+ vs opponent 5k+";
    wait_for(&mut h, "synced game not shown", |h| {
        h.query_by_label_contains(players).is_some()
    });
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes.len(),
        4,
        "sync must preserve the open review"
    );
    h.get_by_label_contains(players).click();
    wait(&mut h, |app| {
        app.state.document.as_ref().unwrap().mainline.len() == 4
    });
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes[0].property("PW"),
        Some("example")
    );
    assert_eq!(
        vault.0.lock().unwrap().get("example").map(String::as_str),
        Some("test-secret")
    );
}

#[test]
fn games_library_filters_synced_games_and_opens_the_selected_record_directly() {
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (
                200,
                support::sgf(if r.url.contains("game2") { 2 } else { 1 }),
            )
        } else {
            (200, support::page(1, 1, &[2, 1]))
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let state = sync(temp.path(), &fixture, vault.clone());
    let mut h = desktop(temp.path(), &fixture, vault, None, 860.);
    h.get_by_label("Games").click();
    select_all_time(&mut h);
    let first = review_row(&state.games[1]);
    let second = review_row(&state.games[0]);
    wait_for_label(&mut h, &second);
    if let Some(path) = std::env::var_os("KATASTRO_PANDANET_UI_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    assert!(h.query_by_label(&first).is_some());
    assert!(
        h.query_by_label("Pandanet password").is_none(),
        "browsing games should not require opening account settings"
    );
    h.get_by_label("Search synced games").focus();
    h.get_by_label("Search synced games")
        .type_text(state.games[1].date.split_whitespace().next().unwrap());
    h.run_steps(3);
    assert!(
        h.query_by_label(&second).is_none(),
        "search must filter the game list"
    );
    h.get_by_label(&first).click();
    wait(&mut h, |app| app.state.document.is_some());
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes[0].property("DT"),
        Some("2026-11-01")
    );
    assert_eq!(h.state().state.document.as_ref().unwrap().mainline.len(), 4);
}

// Benefit: compare one field at a time and open the intended game after sorting.
// A cosmetic header with unchanged row order, text-sorted margins, or a
// click handler tied to the old row index would break this workflow.
#[test]
fn library_table_sorts_every_column_and_opens_the_correct_game_after_filtering() {
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            let id = r
                .url
                .split("v1=game")
                .nth(1)
                .unwrap()
                .split('&')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            return (200, support::sgf(id));
        }
        let rows = [
            (1, "example 10k", "Zebra 1d", "Black won by 10.5"),
            (2, "alpha 2k", "example 6k", "White won by 2.5"),
            (3, "example 1d", "beta 10k", "White won by resign"),
            (4, "Delta 5k", "example 3k", "Black won by time"),
            (5, "example 2k", "omega 2d", "Game unfinished"),
        ];
        let mut html = support::page(1, 1, &[]);
        for (id, white, black, result) in rows {
            let page = support::page(1, 1, &[id]);
            let row = page
                .split("<tr><td>O")
                .nth(1)
                .unwrap()
                .split("</tr>")
                .next()
                .unwrap();
            let row = format!("<tr><td>O{row}</tr>")
                .replace("example 6k+", white)
                .replace("opponent 5k+", black)
                .replace("White won by resign", result);
            html = html.replacen("</table>", &format!("{row}</table>"), 1);
        }
        (200, html)
    });
    let temp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let state = sync(temp.path(), &fixture, vault.clone());
    let games: Vec<_> = state.games.iter().rev().collect();
    let labels: Vec<_> = games.iter().map(|game| review_row(game)).collect();
    let mut h = desktop(temp.path(), &fixture, vault, None, 860.);
    h.get_by_label("Games").click();
    select_all_time(&mut h);
    wait_for_label(&mut h, &labels[0]);
    h.run_steps(3);
    let order = |h: &Harness<'_, DesktopApp>| {
        let mut rows: Vec<_> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| (i + 1, h.get_by_label(label).rect().center().y))
            .collect();
        rows.sort_by(|a, b| a.1.total_cmp(&b.1));
        rows.into_iter().map(|r| r.0).collect::<Vec<_>>()
    };
    assert_eq!(order(&h), [5, 4, 3, 2, 1], "default is newest first");
    let won_dates = h.query_all_by_label_contains("You won on ").count();
    assert_eq!(
        won_dates, 2,
        "only the synced account's two wins get trophies"
    );
    for day in [3, 4] {
        let trophy = h
            .get_by_label(&format!("You won on {}", games[day - 1].date))
            .rect();
        assert!(h.output().shapes.iter().any(|shape| matches!(&shape.shape,
            eframe::egui::Shape::Path(path) if path.closed && path.fill.r() > 180 && path.fill.g() > 120 && path.fill.b() < 80
            && !path.points.is_empty() && path.points.iter().all(|p| trophy.contains(*p))
        )), "a visible gold trophy must be painted on a win");

        assert!(
            (trophy.center().y - h.get_by_label(&labels[day - 1]).rect().center().y).abs() < 1.
        );
    }

    assert!(
        h.query_by_label("Sort by White rank").is_none(),
        "rank belongs beside the White player name"
    );
    assert!(
        h.query_by_label("Sort by Black rank").is_none(),
        "rank belongs beside the Black player name"
    );

    for (column, ascending) in [
        ("Date", vec![1, 2, 3, 4, 5]),
        ("White", vec![2, 4, 5, 3, 1]),
        ("Black", vec![3, 4, 2, 5, 1]),
        ("Winner", vec![4, 1, 3, 2, 5]),
        ("Win condition", vec![2, 1, 3, 4, 5]),
    ] {
        h.get_by_label(&format!("Sort by {column}")).click();
        h.run_steps(3);
        assert_eq!(order(&h), ascending, "ascending {column}");
        h.get_by_label(&format!("Sort by {column}")).click();
        h.run_steps(3);
        assert_eq!(
            order(&h),
            ascending.into_iter().rev().collect::<Vec<_>>(),
            "descending {column}"
        );
    }
    // Distinct cells, aligned under distinct headings; preserve unknown results.
    for text in [
        "10.5 points",
        "2.5 points",
        "Resignation",
        "Time",
        "Game unfinished",
        "Zebra 1d",
        "example 10k",
    ] {
        assert!(
            h.query_all_by_label(text).next().is_some(),
            "missing table value: {text}"
        );
    }
    assert!(h.query_by_label("Black won by 10.5").is_none());
    for (text, column, next) in [
        ("alpha 2k", "White", "Black"),
        ("Zebra 1d", "Black", "Winner"),
    ] {
        let cell = h.get_by_label(text).rect();
        assert!(cell.left() >= h.get_by_label(&format!("Sort by {column}")).rect().left());
        assert!(
            cell.right() <= h.get_by_label(&format!("Sort by {next}")).rect().left(),
            "{text} must occupy its own column"
        );
    }
    assert!(
        (h.get_by_label("10.5 points").rect().center().y
            - h.get_by_label(&labels[0]).rect().center().y)
            .abs()
            < 1.
    );

    if let Some(path) = std::env::var_os("KATASTRO_PANDANET_TABLE_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    h.get_by_label("Search synced games").focus();
    h.get_by_label("Search synced games")
        .type_text("resignation");
    h.run_steps(3);
    assert!(h.query_by_label(&labels[0]).is_none());
    assert!(h.query_by_label(&labels[2]).is_some());
    h.get_by_label("Resignation").click();
    wait(&mut h, |app| app.state.document.is_some());
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes[0].property("DT"),
        Some("2026-11-03")
    );
    assert_eq!(h.state().state.document.as_ref().unwrap().mainline.len(), 4);
}

// Benefit: making the library taller reveals more games, including with settings
// open. A fixed body-height cap leaves empty space instead of usable rows.
#[test]
fn library_table_uses_extra_window_height_to_show_and_open_more_games() {
    use eframe::egui::vec2;
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            let id = r
                .url
                .split("v1=game")
                .nth(1)
                .unwrap()
                .split('&')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            (200, support::sgf(id))
        } else {
            (
                200,
                support::page(1, 1, &(1..=20).rev().collect::<Vec<_>>()),
            )
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let state = sync(temp.path(), &fixture, vault.clone());
    let mut h = desktop(temp.path(), &fixture, vault, None, 1500.);
    h.get_by_label("Games").click();
    select_all_time(&mut h);
    wait_for_label(&mut h, &review_row(&state.games[0]));
    h.run_steps(3);
    let before = h.query_all_by_label_contains("Review 2026/").count();
    let window = h.get_by_label("Game library").rect();
    h.run_steps(10);
    assert!(
        (h.get_by_label("Game library").rect().height() - window.height()).abs() < 1.,
        "the library must not grow by itself between frames"
    );

    let from = window.right_bottom() - vec2(2., 2.);
    let to = from + vec2(0., 600.);
    h.hover_at(from);
    h.run_steps(2);
    h.drag_at(from);
    h.step();
    h.hover_at(to);
    h.step();
    h.drop_at(to);
    h.run_steps(4);
    assert!(
        h.query_all_by_label_contains("Review 2026/").count() >= before + 6,
        "a taller window must reveal more games instead of blank space"
    );
    h.get_by_label("Sync settings").click();
    h.run_steps(3);
    assert!(
        h.query_all_by_label_contains("Review 2026/").count() >= 16,
        "settings must not impose a separate short table cap"
    );
    if let Some(path) = std::env::var_os("KATASTRO_PANDANET_TALL_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    h.get_by_label(&review_row(&state.games[15])).click();
    wait(&mut h, |app| app.state.document.is_some());
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes[0].property("DT"),
        Some("2026-11-05")
    );
}

#[test]
fn library_opens_training_setup_with_the_synced_account_and_returns_to_review() {
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("mode=sgf") {
            (200, support::sgf(1))
        } else {
            (200, support::page(1, 1, &[1]))
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let state = sync(temp.path(), &fixture, vault.clone());
    let mut h = desktop(temp.path(), &fixture, vault, None, 860.);
    h.get_by_label("Games").click();
    select_all_time(&mut h);
    wait_for_label(&mut h, &review_row(&state.games[0]));
    h.get_by_label("Train from these games").click();
    h.run_steps(3);
    assert!(h.query_by_label("Start training").is_some());
    assert!(
        h.query_by_label("1 games available in your library")
            .is_some()
    );
    assert!(h.query_by_label("Game library").is_none());
    h.get_by_label("Back to review").click();
    h.run_steps(3);
    assert!(h.query_by_label("Games").is_some());
    assert!(h.query_by_label("Start training").is_none());
}
