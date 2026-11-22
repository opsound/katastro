#![cfg(feature = "desktop")]
#[path = "support/archive_ui.rs"]
mod archive_ui;
#[path = "support/pandanet/mod.rs"]
mod support;
use archive_ui::{archive_desktop, select_all_time, wait_for, wait_for_label};
use eframe::egui::vec2;
use egui_kittest::kittest::Queryable;
use katastro::archive::{Options, Server};
use std::sync::Arc;

// Benefit: both archives share a sortable library but can be filtered and
// trained independently, even when the user's server names differ.
#[test]
fn server_filter_opens_fox_original_and_training_uses_fox_account() {
    let fixture = support::Fixture::new(|r| {
        if r.url.contains("QueryUserInfoPanel") {
            (
                200,
                r#"{"result":0,"uid":"42","username":"foxplayer","hide_game_record":0}"#.into(),
            )
        } else if r.url.contains("FetchChessList") {
            (200,r#"{"result":0,"dstuid":"42","chesslist":[{"chessid":"1","blackuid":42,"whiteuid":55,"starttime":"2026-11-02 12:00:00"}]}"#.into())
        } else if r.url.contains("FetchChess") {
            (200,serde_json::json!({"result":0,"chessid":"1","chess":"(;SZ[9]PB[foxplayer]BR[3级]PW[rival]WR[2段]RE[B+R];B[cc];W[gg])"}).to_string())
        } else if r.url.contains("mode=sgf") {
            (200, support::sgf(1))
        } else {
            (200, support::page(1, 1, &[1]))
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut options = vec![
        Options::new(root.join("Pandanet")),
        Options::fox(root.join("Fox")),
    ];
    for option in &mut options {
        option.endpoint = fixture.endpoint.clone();
        option.credentials = Arc::new(support::Vault::default());
        let (username, password) = if option.server == Server::Fox {
            ("foxplayer", "")
        } else {
            ("example", "test-secret")
        };
        support::seed_account(option, username, password);
    }
    let mut h = archive_desktop(root, options, vec2(1200., 900.));
    h.get_by_label("Games").click();
    select_all_time(&mut h);
    let panda = "Review 2026/11/01 12:00: example 6k+ vs opponent 5k+";
    let fox = "Review 2026/11/02 12:00:00: rival 2d vs foxplayer 3k";
    wait_for_label(&mut h, panda);
    h.get_by_label("All servers");
    wait_for_label(&mut h, fox);
    h.get_by_label("You won on 2026/11/02 12:00:00");
    h.get_by_label("Sort by Server").click();
    h.run_steps(3);
    assert!(h.get_by_label(fox).rect().top() < h.get_by_label(panda).rect().top());
    h.get_by_label("Sort by Server").click();
    h.run_steps(3);
    assert!(h.get_by_label(panda).rect().top() < h.get_by_label(fox).rect().top());
    if let Some(path) = std::env::var_os("KATASTRO_FOX_UI_PREVIEW") {
        h.render().unwrap().save(path).unwrap();
    }

    h.get_by_label("Fox games").click();
    h.run_steps(3);
    h.get_by_label(fox);
    assert!(h.query_by_label(panda).is_none());
    h.get_by_label("Pandanet games").click();
    h.run_steps(3);
    h.get_by_label(panda);
    assert!(h.query_by_label(fox).is_none());
    h.get_by_label("All servers").click();
    h.run_steps(3);
    h.get_by_label(fox);
    h.get_by_label(panda);
    h.get_by_label("Fox games").click();
    h.run_steps(3);
    h.get_by_label("Train from these games").click();
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Training player name").value().as_deref(),
        Some("foxplayer")
    );
    h.get_by_label("1 games available in your library");
    h.get_by_label("Back to review").click();
    h.run_steps(3);
    h.get_by_label("Games").click();
    h.run_steps(3);
    h.get_by_label(fox).click();
    wait_for(&mut h, "the Fox review did not open", |h| {
        h.state().state.document.is_some()
    });
    assert_eq!(
        h.state().state.document.as_ref().unwrap().nodes[0].property("PB"),
        Some("foxplayer")
    );
}

#[test]
fn fox_settings_connect_with_only_a_username_and_keep_pandanet_separate() {
    let fixture = support::Fixture::new(|r| {
        assert!(r.body.is_empty());
        let response = if r.url.contains("QueryUserInfoPanel") {
            serde_json::json!({"result":0,"uid":"42","username":"foxplayer","hide_game_record":0})
        } else if r.url.contains("FetchChessList") {
            serde_json::json!({"result":0,"dstuid":"42","chesslist":[{"chessid":"1","blackuid":42,"whiteuid":55,"starttime":"2026-11-02 12:00:00"}]})
        } else {
            serde_json::json!({"result":0,"chessid":"1","chess":"(;SZ[9]PB[foxplayer]PW[rival]RE[B+R];B[cc])"})
        };
        (200, response.to_string())
    });
    let root = tempfile::tempdir().unwrap();
    let vault = Arc::new(support::Vault::default());
    let mut fox = Options::fox(root.path().join("Fox"));
    fox.endpoint = fixture.endpoint.clone();
    fox.credentials = vault.clone();
    let panda = Options::new(root.path().join("Pandanet"));
    let mut h = archive_desktop(root.path(), vec![panda, fox], vec2(1200., 900.));
    h.get_by_label("Games").click();
    h.run_steps(3);
    h.get_by_label("Sync settings").click();
    h.run_steps(3);
    h.get_by_label("Fox sync").click();
    h.run_steps(3);
    assert!(h.query_by_label("Pandanet password").is_none());
    assert!(h.query_by_label("Remember login in Keychain").is_none());
    h.get_by_label("Sync Fox now");
    h.get_by_label("Fox username").focus();
    h.get_by_label("Fox username").type_text("foxplayer");
    h.run_steps(3);
    h.get_by_label("Connect and sync").click();
    select_all_time(&mut h);
    wait_for_label(&mut h, "Review 2026/11/02 12:00:00: rival vs foxplayer");
    assert!(vault.0.lock().unwrap().is_empty());
    h.get_by_label("Pandanet sync").click();
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Pandanet username").value().as_deref(),
        Some("")
    );
    h.get_by_label("Pandanet password");
    h.get_by_label("Fox sync").click();
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Fox username").value().as_deref(),
        Some("foxplayer")
    );
}
