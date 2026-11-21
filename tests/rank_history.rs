use katastro::archive::{
    Game, Server, Snapshot,
    rank_history::{date_label, rank_label, series},
};

fn snapshot(server: Server, username: &str, rows: &[(&str, &str, &str)]) -> Snapshot {
    Snapshot {
        server,
        username: username.into(),
        games: rows
            .iter()
            .enumerate()
            .map(|(id, &(date, black, white))| Game {
                id: id.to_string(),
                date: date.into(),
                black: black.into(),
                white: white.into(),
                result: String::new(),
                path: Default::default(),
            })
            .collect(),
        ..Default::default()
    }
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
            ("2024/03/01 00:00:01", "Fox Player 9d", "Blue 2k"),
            ("2024/03/01 02:30:31", "Rival 8p", "Fox Player 1p"),
        ],
    );
    let ogs = snapshot(
        Server::Ogs,
        "Île 3d",
        &[("2024/03/01 00:00:00", "île 3d 4K+", "Rival 8k")],
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
    assert_eq!(date_label(panda[2].timestamp), "2024-02-29");
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
