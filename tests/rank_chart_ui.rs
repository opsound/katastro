#![cfg(feature = "desktop")]
#[path = "support/archive_ui.rs"]
mod archive_ui;
#[path = "support/pandanet/mod.rs"]
mod support;

use archive_ui::{archive_desktop, wait_for};
use eframe::egui::{self, Color32, Pos2, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use katastro::{
    archive::{Options, Service},
    desktop::DesktopApp,
};
use rusqlite::params;
use std::{path::Path, sync::Arc};

// Reopen a pre-existing library so the graph and the rows use the same saved metadata.
fn seed(option: &Options, names: &[(&str, &str, &str)]) {
    let user = "local player";
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
        "INSERT INTO config(singleton,username,remember,automatic) VALUES(1,?1,0,0)",
        [user],
    )
    .unwrap();
    for (index, (date, white, black)) in names.iter().enumerate() {
        let file = format!("{index}.sgf");
        std::fs::write(
            option.root.join(&file),
            format!("(;SZ[9]PB[{black}]PW[{white}];B[aa])"),
        )
        .unwrap();
        db.execute(
            "INSERT INTO games VALUES(?1,?2,?3,?4,?5,'B+R',?6,'')",
            params![user, index.to_string(), date, white, black, file],
        )
        .unwrap();
    }
}

type GameRow<'a> = (&'a str, &'a str, &'a str);

fn desktop(
    root: &Path,
    games: [&[GameRow<'_>]; 3],
    size: egui::Vec2,
) -> Harness<'static, DesktopApp> {
    let mut options = vec![
        Options::new(root.join("Pandanet")),
        Options::fox(root.join("Fox")),
        Options::ogs(root.join("OGS")),
    ];
    for (option, rows) in options.iter_mut().zip(games) {
        option.credentials = Arc::new(support::Vault::default());
        seed(option, rows);
    }
    let mut h = archive_desktop(root, options, size);
    h.get_by_label("Games").click();
    let count: usize = games.iter().map(|rows| rows.len()).sum();
    let expected = format!("{count} synced games");
    wait_for(&mut h, "library snapshots never arrived", |h| {
        h.query_by_label(&expected).is_some()
    });
    h.run_steps(3);
    h
}

fn legend_color(h: &Harness<'_, DesktopApp>, legend: &str) -> Color32 {
    h.output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.text() == legend => {
                Some(t.galley.job.sections[0].format.color)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing server and latest rank in legend: {legend}"))
}

fn toggle_history(h: &mut Harness<'_, DesktopApp>) {
    h.get_by_label("Show rank history").click();
    h.run_steps(3);
}

fn visible_game_rows(h: &Harness<'_, DesktopApp>) -> usize {
    h.output().shapes.iter().filter(|s| matches!(&s.shape, Shape::Text(t)
        if t.galley.text().starts_with("2026/11/") && s.clip_rect.contains_rect(t.galley.rect.translate(t.pos.to_vec2())))).count()
}

fn points(h: &Harness<'_, DesktopApp>, color: Color32) -> Vec<Pos2> {
    let rect = h.get_by_label("Rank history chart").rect();
    let mut points: Vec<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Circle(c) if c.fill == color && rect.contains(c.center) => Some(c.center),
            _ => None,
        })
        .collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x));
    points
}

// Benefit: the three accounts share real elapsed time and an ordered rank ladder.
// Wrong-axis plots, inter-server joins, hidden single observations and unreadable ticks
// would otherwise make comparisons unreliable.
#[test]
fn games_overlays_three_rank_histories_and_filters_them_with_the_table() {
    let temp = tempfile::tempdir().unwrap();
    let games: [&[GameRow<'_>]; 3] = [
        &[
            ("2026/11/01 12:00", "local player 4k", "a 7k"),
            ("2026/11/03 12:00", "b 6k", "local player 2k"),
            ("2026/11/09 12:00", "local player 1d", "c 4k"),
        ],
        &[
            ("2026/11/01 12:00", "local player 1d", "d 3k"),
            ("2026/11/03 12:00", "e 5k", "local player 1k"),
            ("2026/11/09 12:00", "local player 3k", "f 1k"),
        ],
        &[("2026/11/03 12:00", "local player 1k", "g 5k")],
    ];
    let mut h = desktop(temp.path(), games, egui::vec2(1200., 900.));
    assert!(
        h.query_by_label("Rank history chart").is_none(),
        "Games should give the space to the game list until rank history is expanded"
    );
    let collapsed_rows = visible_game_rows(&h);
    toggle_history(&mut h);
    let plot = h.get_by_label("Rank history chart").rect();
    let panda = legend_color(&h, "Pandanet  1d");
    let fox = legend_color(&h, "Fox  3k");
    let ogs = legend_color(&h, "OGS  1k");
    assert!(panda != fox && panda != ogs && fox != ogs);
    let a = points(&h, panda);
    let b = points(&h, fox);
    let c = points(&h, ogs);
    assert_eq!((a.len(), b.len(), c.len()), (3, 3, 1));
    for (color, markers) in [(panda, &a), (fox, &b)] {
        assert!(
            h.output()
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, Shape::Path(p)
            if p.stroke.color == egui::epaint::ColorMode::Solid(color) && p.points == *markers)),
            "a continuous trace must join the dated observations for its own server"
        );
    }
    assert_eq!(a[0].x, b[0].x);
    assert_eq!(a[2].x, b[2].x);
    assert!(
        (a[1].x - a[0].x - (a[2].x - a[0].x) / 4.).abs() < 1.0,
        "dates must use elapsed days"
    );
    assert!(a[0].y > a[1].y && a[1].y > a[2].y && b[0].y < b[1].y && b[1].y < b[2].y);
    assert_eq!(a[2].y, b[0].y);
    assert_eq!(
        b[1], c[0],
        "a single point at an overlapping server date and rank must be drawn"
    );
    let overlapping: Vec<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Circle(marker) if marker.center == c[0] => Some((marker.fill, marker.radius)),
            _ => None,
        })
        .collect();
    assert!(
        overlapping.len() == 2
            && overlapping[0].0 == fox
            && overlapping[1].0 == ogs
            && overlapping[0].1 > overlapping[1].1,
        "both servers need to remain visible at their coincident point"
    );
    for (expected, y) in [("1d", a[2].y), ("1k", b[1].y), ("3k", b[2].y)] {
        assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if {
            let r = t.galley.rect.translate(t.pos.to_vec2());
            t.galley.text()==expected && s.clip_rect.contains_rect(r) && r.right() <= plot.left()+1. && (r.center().y-y).abs()<1.5
        })), "missing rank tick {expected} at recorded height {y}");
    }
    let mut dates: Vec<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.text().starts_with("2026-") => {
                let r = t.galley.rect.translate(t.pos.to_vec2());
                (s.clip_rect.contains_rect(r)
                    && r.top() >= plot.bottom() - 1.
                    && r.top() < plot.bottom() + 30.)
                    .then_some(r)
            }
            _ => None,
        })
        .collect();
    dates.sort_by(|a, b| a.left().total_cmp(&b.left()));
    assert!(
        dates.len() >= 2
            && dates
                .windows(2)
                .all(|pair| pair[0].right() < pair[1].left()),
        "calendar labels must be visible and legible"
    );
    let expanded_rows = visible_game_rows(&h);
    assert!(
        expanded_rows >= 5,
        "five complete rows must fit beneath the plot"
    );
    assert!(
        collapsed_rows > expanded_rows,
        "hiding the chart must reveal more games"
    );
    if let Some(path) = std::env::var_os("KATASTRO_RANK_HISTORY_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }
    h.hover_at(a[1]);
    h.run_steps(3);
    assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if
        t.galley.text().contains("Pandanet  2k") && t.galley.text().contains("2026-11-03 12:00")
    )), "hovering a game rank must reveal the server, recorded rank, and timestamp");
    toggle_history(&mut h);
    assert!(h.query_by_label("Rank history chart").is_none());
    assert_eq!(visible_game_rows(&h), collapsed_rows);
    toggle_history(&mut h);
    h.get_by_label("OGS games").click();
    h.run_steps(3);
    assert_eq!(points(&h, ogs).len(), 1);
    assert!(points(&h, fox).is_empty() && points(&h, panda).is_empty());
    assert_eq!(h.query_all_by_label_contains("Review 2026/11/").count(), 1);
    let intraday: std::collections::HashSet<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.text().contains("2026-11-03 ") => Some(t.galley.text()),
            _ => None,
        })
        .collect();
    assert!(
        intraday.len() >= 2,
        "intraday ticks must distinguish the time of day"
    );
    h.get_by_label("Sync settings").click();
    h.run_steps(3);
    assert!(h.query_by_label("Rank history chart").is_none());
}

#[test]
fn missing_account_ranks_leave_the_game_list_available() {
    let temp = tempfile::tempdir().unwrap();
    let unranked = [("2026/11/03 12:00", "local player", "rival 9d")];
    let mut h = desktop(temp.path(), [&unranked; 3], egui::vec2(900., 680.));
    assert!(
        h.query_by_label("No recorded player ranks in these games.")
            .is_none()
    );
    toggle_history(&mut h);
    h.get_by_label("No recorded player ranks in these games.");
    assert!(h.query_by_label("Rank history chart").is_none());
    assert_eq!(h.query_all_by_label_contains("Review 2026/11/").count(), 3);
}

#[test]
fn close_game_times_have_a_readable_time_scale_at_compact_width() {
    let temp = tempfile::tempdir().unwrap();
    let records = [
        ("2026/11/03 12:15:04", "local player 3k", "a 2k"),
        ("2026/11/03 12:15:08", "local player 3k", "b 4k"),
    ];
    let mut h = desktop(temp.path(), [&[], &[], &records], egui::vec2(900., 680.));
    toggle_history(&mut h);
    let plot = h.get_by_label("Rank history chart").rect();
    let mut times: Vec<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.text().starts_with("2026-11-03 ") => {
                let rect = t.galley.rect.translate(t.pos.to_vec2());
                (s.clip_rect.contains_rect(rect) && rect.top() >= plot.bottom() - 1.)
                    .then_some((rect, t.galley.text()))
            }
            _ => None,
        })
        .collect();
    times.sort_by(|a, b| a.0.left().total_cmp(&b.0.left()));
    assert!(
        times.len() >= 2
            && times
                .windows(2)
                .all(|p| p[0].0.right() < p[1].0.left() && p[0].1 != p[1].1),
        "close game times need distinct, unclipped date/time labels: {times:?}"
    );
}
