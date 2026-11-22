use katastro::archive::{
    Keychain, Server, Service, Snapshot,
    rank_history::{date_label, rank_label, series},
};
use rusqlite::params;
use std::sync::Arc;
use time::macros::datetime;

fn snapshot(server: Server, username: &str, rows: &[(&str, &str, &str)]) -> Snapshot {
    let root = tempfile::tempdir().unwrap();
    let open = || {
        Service::open_for(
            root.path(),
            "http://127.0.0.1/",
            Arc::new(Keychain::default()),
            server,
        )
        .unwrap()
    };
    drop(open());
    let db = rusqlite::Connection::open(root.path().join("sync.sqlite")).unwrap();
    db.execute(
        "INSERT INTO config(singleton, username, remember, automatic) VALUES(1, ?1, 0, 0)",
        [username],
    )
    .unwrap();
    for (id, (date, black, white)) in rows.iter().enumerate() {
        db.execute(
            "INSERT INTO games VALUES(?1,?2,?3,?4,?5,'B+R','game.sgf','')",
            params![username, id.to_string(), date, white, black],
        )
        .unwrap();
    }
    open().snapshot()
}

// The graph follows the synced player across colors and across the kyu/dan
// boundary. Reversing archive order, plotting opponents, or indexing games at
// even intervals would put the promotion in the wrong place.
#[test]
fn archive_accounts_follow_their_own_ranks_on_one_calendar_scale() {
    let panda = snapshot(
        Server::Pandanet,
        "Blue",
        &[
            ("2024/03/01 00:00", "Rival 9d", "Blue 1d*"),
            ("2024/02/28 12:00", "Blue 2k+", "Rival 3d"),
            ("2024/02/29 12:00", "Rival 7d", "BLUE 1k-"),
            ("2024/02/28 10:00", "Blue 2k", "Rival 3d"),
            ("2024/02/29 23:00", "Blue 1k+", "Rival 3d"),
        ],
    );
    let fox = snapshot(
        Server::Fox,
        "Fox Player",
        &[
            ("2024/02/29 23:00:01", "Fox Player 9d", "Blue 2k"),
            ("2024/03/01 01:30:31", "Rival 8p", "Fox Player 1p"),
        ],
    );
    let ogs = snapshot(
        Server::Ogs,
        "Île 3d",
        &[("2024/02/29 15:00:00", "île 3d 4K+", "Rival 8k")],
    );

    let panda = series(&panda);
    assert_eq!(
        panda
            .iter()
            .map(|p| rank_label(p.level))
            .collect::<Vec<_>>(),
        ["2k", "2k+", "1k", "1k+", "1d"]
    );
    assert!(
        panda
            .windows(2)
            .all(|pair| pair[1].level - pair[0].level == 1)
    );
    assert_eq!(panda[2].timestamp - panda[1].timestamp, 86_400);
    assert_eq!(panda[4].timestamp - panda[2].timestamp, 43_200);
    assert_eq!(
        panda[2].timestamp,
        datetime!(2024-02-29 03:00 UTC).unix_timestamp()
    );
    let fox = series(&fox);
    assert_eq!(
        fox.iter().map(|p| rank_label(p.level)).collect::<Vec<_>>(),
        ["9d", "1p"]
    );
    assert_eq!(fox[0].timestamp - panda[4].timestamp, 1);
    assert_eq!(fox[1].timestamp - fox[0].timestamp, 9_030);
    assert_eq!(fox[1].level - fox[0].level, 2);
    let ogs = series(&ogs);
    assert_eq!(ogs.len(), 1);
    assert_eq!(rank_label(ogs[0].level), "4k");
    assert_eq!(ogs[0].timestamp, panda[4].timestamp);
}

// Only a recorded individual rank at a real date is evidence. A handle that
// ends in a rank token is still a handle; its opponent or team cannot supply it.
#[test]
fn missing_ranks_dates_and_ambiguous_player_names_never_become_chart_points() {
    let games = snapshot(
        Server::Ogs,
        "River 1d",
        &[
            ("2026/06/30", "River 1d", "Opponent 9p"),
            ("2026/06/29", "River 1d NR", "Opponent 7d"),
            ("2026/06/28", "River 1d ?", "Opponent 6d"),
            ("2026/06/27", "River 1d Jr 2k", "Another 2d"),
            ("2026/06/26", "River 1d:Partner 4k", "Opponent 2d"),
            ("2026/06/25", "River 1d 0k", "Opponent 2d"),
            ("2026/06/24", "River 1d 10d", "Opponent 2d"),
            ("2026/06/23", "River 1d 4d", "River 1d"),
            ("2026/06/31", "River 1d 4d", "Opponent 2d"),
            ("2026/06/24 99:01", "River 1d 4d", "Opponent 2d"),
            ("2026/06/24 01:01:00 trailing", "River 1d 4d", "Opponent 2d"),
            ("unknown date", "River 1d 4d", "Opponent 2d"),
            ("2026-06-03", "Opponent 6d", "River 1d 3d"),
        ],
    );
    let points = series(&games);
    assert_eq!(points.len(), 1);
    assert_eq!(rank_label(points[0].level), "3d");
    assert_eq!(date_label(points[0].timestamp), "2026-06-03");
}

// Read both seasons from a persisted index. The local offset can differ between them.
#[test]
fn existing_archives_agree_on_local_play_times_and_preserve_date_only_games() {
    use time::{OffsetDateTime, UtcOffset, macros::format_description};
    let local = |utc: OffsetDateTime| {
        utc.to_offset(UtcOffset::local_offset_at(utc).unwrap_or(UtcOffset::UTC))
            .format(format_description!("[year]/[month]/[day] [hour]:[minute]"))
            .unwrap()
    };
    let winter = local(datetime!(2026-01-10 07:30 UTC));
    let summer = local(datetime!(2026-07-10 07:30 UTC));
    for (server, clock) in [
        (Server::Pandanet, "16:30"),
        (Server::Fox, "15:30"),
        (Server::Ogs, "07:30"),
    ] {
        let games = snapshot(
            server,
            "member",
            &[
                (&format!("2026/01/10 {clock}"), "member", "winter"),
                (&format!("2026/07/10 {clock}"), "member", "summer"),
                ("2026-01-10", "member", "date only"),
                ("", "member", "undated"),
            ],
        )
        .games;
        for (opponent, expected) in [
            ("winter", winter.as_str()),
            ("summer", summer.as_str()),
            ("date only", "2026-01-10"),
            ("undated", ""),
        ] {
            let game = games.iter().find(|g| g.white == opponent).unwrap();
            assert_eq!(game.date, expected, "{server:?} {opponent}");
        }
    }
}
