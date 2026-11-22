#![cfg(feature = "desktop")]
#[path = "support/archive_ui.rs"]
mod archive_ui;
#[path = "support/pandanet/mod.rs"]
mod support;

use archive_ui::{archive_desktop, wait_for};
use eframe::egui::vec2;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use katastro::{archive::Options, desktop::DesktopApp};
use serde_json::json;
use std::{path::Path, sync::Arc};

const OGS_ROW: &str = "vs ogsplayer";
const FOX_ROW: &str = "vs foxplayer";

fn desktop(root: &Path) -> (Harness<'static, DesktopApp>, support::Fixture) {
    let http = support::Fixture::new(|r| {
        assert!(r.body.is_empty(), "public servers only need GET requests");
        let path = r.url.split('?').next().unwrap().trim_end_matches('/');
        let body = match path {
            "/api/v1/players" => {
                json!({"count":1,"next":null,"results":[{"id":42,"username":"ogsplayer"}]})
            }
            "/api/v1/players/42/games" => json!({"count":1,"next":null,"results":[{
                "id":100,"black":42,"white":57,"players":{"black":{"id":42,"username":"ogsplayer"},"white":{"id":57,"username":"rival"}},
                "started":"2026-01-01T10:00:00Z","ended":"2026-01-01T10:30:00Z",
                "black_lost":false,"white_lost":true,"outcome":"Resignation"
            }]}),
            "/api/v1/games/100/sgf" => {
                return (
                    200,
                    "(;FF[4]GM[1]SZ[9]PB[ogsplayer]PW[rival]RE[B+R];B[hh];W[gg])".into(),
                );
            }
            "/cgi/QueryUserInfoPanel" => {
                json!({"result":0,"uid":"11","username":"foxplayer","hide_game_record":0})
            }
            "/yehuDiamond/chessbook_local/YHWQFetchChessList" => {
                json!({"result":0,"dstuid":"11","chesslist":[{"chessid":"100","blackuid":11,"whiteuid":21,"starttime":"2026-01-02 10:30:00"}]})
            }
            "/yehuDiamond/chessbook_local/YHWQFetchChess" => {
                json!({"result":0,"chessid":"100","chess":"(;FF[4]GM[1]SZ[9]PB[foxplayer]PW[rival]RE[W+R];B[aa];W[bb])"})
            }
            _ => return (404, "Unknown fixture route".into()),
        };
        (200, body.to_string())
    });
    let vault = Arc::new(support::Vault::default());
    let mut fox = Options::fox(root.join("Fox"));
    fox.endpoint = http.endpoint.clone();
    fox.credentials = vault.clone();
    assert_eq!(
        support::seed_account(&fox, "foxplayer", "").games[0].id,
        "100"
    );
    let mut ogs = Options::ogs(root.join("OGS"));
    ogs.endpoint = http.endpoint.split("/cgi-bin").next().unwrap().to_owned();
    ogs.credentials = vault.clone();
    let mut pandanet = Options::new(root.join("Pandanet"));
    pandanet.endpoint = http.endpoint.clone();
    pandanet.credentials = vault;
    (
        archive_desktop(root, vec![pandanet, fox, ogs], vec2(1200., 900.)),
        http,
    )
}

fn connect(h: &mut Harness<'_, DesktopApp>) {
    h.get_by_label("OGS username").focus();
    h.get_by_label("OGS username").type_text("ogsplayer");
    h.run();
    h.get_by_label("Connect and sync").click();
    wait_for(h, "the OGS game never appeared after connecting", |h| {
        h.query_by_label_contains(OGS_ROW).is_some() && h.query_by_label("Sync OGS now").is_some()
    });
}

#[test]
fn an_ogs_only_desktop_opens_the_game_library_and_its_connection_settings() {
    let temp = tempfile::tempdir().unwrap();
    let mut ogs = Options::ogs(temp.path().join("OGS"));
    ogs.endpoint = "http://127.0.0.1:1/".into();
    ogs.credentials = Arc::new(support::Vault::default());
    let mut h = archive_desktop(temp.path(), vec![ogs], vec2(1200., 900.));
    h.get_by_label("Games").click();
    h.run();
    h.query_by_label("OGS games")
        .expect("the only configured archive must be browsable")
        .click();
    h.run();
    assert!(
        h.query_by_label("Pandanet games").is_none() && h.query_by_label("Fox games").is_none()
    );
    h.get_by_label("Sync settings").click();
    h.run();
    h.get_by_label("OGS username").focus();
    h.get_by_label("OGS username").type_text("ogsplayer");
    h.run();
    assert!(
        !h.get_by_label("Connect and sync")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn ogs_settings_connect_using_only_a_username_and_preserve_the_other_accounts() {
    let temp = tempfile::tempdir().unwrap();
    let (mut h, http) = desktop(temp.path());
    h.get_by_label("Games").click();
    h.run();
    h.get_by_label("Sync settings").click();
    h.run();
    h.get_by_label("Pandanet password").focus();
    h.get_by_label("Pandanet password")
        .type_text("synthetic-test-password");
    h.run();
    h.get_by_label("OGS sync").click();
    h.run();
    assert!(
        h.query_by_label("Pandanet password").is_none(),
        "OGS asks for the other server's password"
    );
    assert!(h.query_by_label("Remember login in Keychain").is_none());
    assert!(
        h.get_by_label("Connect and sync")
            .accesskit_node()
            .is_disabled()
    );
    connect(&mut h);
    h.get_by_label("Sync OGS now").click();
    wait_for(&mut h, "manual OGS sync did not refresh the archive", |h| {
        h.query_by_label("Up to date · 1 games · 0 downloaded")
            .is_some()
    });
    let requests = http.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.contains("/api/v1/players/42/games"))
            .count(),
        2
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.contains("/api/v1/games/100/sgf"))
            .count(),
        1
    );
    drop(requests);
    h.get_by_label("Pandanet sync").click();
    h.run();
    assert_eq!(
        h.get_by_label("Pandanet username").value().as_deref(),
        Some("")
    );
    assert!(
        h.get_by_label("Pandanet password")
            .value()
            .unwrap_or_default()
            .is_empty()
    );
    h.get_by_label("OGS sync").click();
    h.run();
    assert_eq!(
        h.get_by_label("OGS username").value().as_deref(),
        Some("ogsplayer")
    );
    h.get_by_label("Disconnect OGS").click();
    wait_for(&mut h, "the OGS account did not disconnect", |h| {
        h.query_by_label("Disconnected; downloaded games are kept")
            .is_some()
    });
    assert!(h.query_by_label_contains(OGS_ROW).is_some());
    assert!(h.query_by_label_contains(FOX_ROW).is_some());
}

#[test]
fn ogs_filter_trains_the_selected_account_and_opens_its_original_when_game_ids_match() {
    let temp = tempfile::tempdir().unwrap();
    let (mut h, _http) = desktop(temp.path());
    h.get_by_label("Games").click();
    h.run();
    wait_for(&mut h, "the seeded Fox game was not loaded", |h| {
        h.query_by_label_contains(FOX_ROW).is_some()
    });
    h.query_by_label("OGS games")
        .expect("the Games window needs an OGS filter")
        .click();
    h.run();
    assert!(h.query_by_label_contains(FOX_ROW).is_none());
    h.get_by_label("Sync settings").click();
    h.run();
    connect(&mut h);
    h.get_by_label("Sync settings").click();
    h.run();
    h.get_by_label("All servers").click();
    h.run();
    h.get_by_label_contains(OGS_ROW);
    h.get_by_label_contains(FOX_ROW);
    h.get_by_label("OGS games").click();
    h.run();
    assert!(h.query_by_label_contains(FOX_ROW).is_none());
    h.get_by_label("Train from these games").click();
    h.run();
    assert_eq!(
        h.get_by_label("Training player name").value().as_deref(),
        Some("ogsplayer")
    );
    h.get_by_label("1 games available in your library");
    h.get_by_label("Back to review").click();
    h.run();
    h.get_by_label("Games").click();
    h.run();
    h.get_by_label_contains(OGS_ROW).click();
    wait_for(&mut h, "the OGS review did not open", |h| {
        h.state().state.document.is_some()
    });
    let loaded = &h.state().state;
    assert_eq!(
        loaded.document.as_ref().unwrap().nodes[0].property("PB"),
        Some("ogsplayer")
    );
    let source = loaded.source.as_ref().unwrap();
    assert!(source.starts_with(temp.path().join("OGS").canonicalize().unwrap()));
    h.get_by_label("Next move").click();
    wait_for(&mut h, "OGS opening move did not reach the board", |h| {
        h.state().state.board.as_ref().is_some_and(|board| {
            board.stone(katastro::Point::new(7, 7)) == Some(katastro::Color::Black)
        })
    });
}
