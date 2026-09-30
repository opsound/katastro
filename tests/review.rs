use katastro::{Color, Point, Review};
use std::{fs, path::Path};
use tempfile::TempDir;

fn review(root: &Path) -> Review {
    Review::new(&root.join("reviews.sqlite"), &root.join("cache.sqlite")).unwrap()
}
fn fixture(root: &Path, sgf: &str) -> std::path::PathBuf {
    let path = root.join("game.sgf");
    fs::write(&path, sgf).unwrap();
    path
}

#[test]
fn variations_survive_reopen_without_replacing_the_played_game() {
    let temp = TempDir::new().unwrap();
    let source = fixture(
        temp.path(),
        "(;GM[1]FF[4]SZ[9]KM[6.5]RU[Japanese]C[bracket \\] and slash \\\\];B[cc];W[gg](;B[cg])(;B[gc]))",
    );
    let original = fs::read(&source).unwrap();
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let mainline = app.document().unwrap().mainline.clone();
    app.select(mainline[1]).unwrap();
    let variation = app.play(Some(Point::new(3, 3))).unwrap();
    let mut immediate = review(temp.path());
    immediate.import(&source).unwrap();
    assert_eq!(
        immediate.document().unwrap().nodes.len(),
        app.document().unwrap().nodes.len(),
        "each accepted move must autosave before later navigation"
    );
    let leaf = app.play(Some(Point::new(4, 4))).unwrap();
    immediate.import(&source).unwrap();
    assert!(
        immediate.document().unwrap().nodes[variation]
            .children
            .contains(&leaf)
    );
    app.select(mainline[1]).unwrap();
    assert_eq!(
        app.play(Some(Point::new(3, 3))).unwrap(),
        variation,
        "replaying an existing child must not duplicate it"
    );
    assert_eq!(app.document().unwrap().mainline, mainline);
    drop(app);
    let mut reopened = review(temp.path());
    reopened.import(&source).unwrap();
    let doc = reopened.document().unwrap();
    assert_eq!(doc.mainline, mainline);
    assert!(doc.nodes[variation].children.contains(&leaf));
    assert_eq!(
        doc.board(leaf).unwrap().stone(Point::new(4, 4)),
        Some(Color::Black)
    );
    assert_eq!(doc.nodes[0].property("C"), Some("bracket ] and slash \\"));
    assert_eq!(
        fs::read(&source).unwrap(),
        original,
        "autosave must leave original SGF intact"
    );
    let exported = temp.path().join("export.sgf");
    reopened.export(&exported).unwrap();
    let mut imported = review(&temp.path().join("second"));
    imported.import(&exported).unwrap();
    assert_eq!(imported.document().unwrap().nodes.len(), doc.nodes.len());
    assert!(
        reopened.export(&source).is_err(),
        "source overwrite must be explicit, never an export default"
    );
}

#[test]
fn captures_setup_and_illegal_moves_are_applied_atomically() {
    let temp = TempDir::new().unwrap();
    let source = fixture(
        temp.path(),
        "(;SZ[5]RU[Japanese]AB[ab][ba][cb]AW[bb]PL[B];B[bc];W[dd])",
    );
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let line = app.document().unwrap().mainline.clone();
    app.select(line[1]).unwrap();
    let board = app.document().unwrap().board(line[1]).unwrap();
    assert_eq!(board.stone(Point::new(1, 1)), None);
    assert_eq!(board.captures, [1, 0]);
    assert_eq!(board.next, Color::White);
    let before = serde_json::to_value(app.document()).unwrap();
    assert!(
        app.play(Some(Point::new(1, 2))).is_err(),
        "occupied intersections must reject the move"
    );
    assert_eq!(serde_json::to_value(app.document()).unwrap(), before);
    app.select(0).unwrap();
    assert_eq!(
        app.document()
            .unwrap()
            .board(0)
            .unwrap()
            .stone(Point::new(1, 1)),
        Some(Color::White)
    );
}

#[test]
fn ko_suicide_and_pass_are_checked_through_review_commands() {
    let temp = TempDir::new().unwrap();
    let source = fixture(
        temp.path(),
        "(;SZ[5]RU[Japanese]AB[ab][ba][cb]AW[bb][ac][cc][bd]PL[B])",
    );
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let capture = app.play(Some(Point::new(1, 2))).unwrap();
    assert!(
        app.play(Some(Point::new(1, 1))).is_err(),
        "immediate ko recapture must be rejected"
    );
    let pass = app.play(None).unwrap();
    assert_eq!(
        app.document().unwrap().board(pass).unwrap().next,
        Color::Black
    );
    app.select(capture).unwrap();
    // A surrounded Black point cannot be filled when no opponent is captured.
    let source = fixture(temp.path(), "(;SZ[5]AW[ab][ba][cb][bc]PL[B])");
    app.import(&source).unwrap();
    assert!(
        app.play(Some(Point::new(1, 1))).is_err(),
        "suicide is illegal under the default Chinese rules"
    );
}

#[test]
fn nested_variations_keep_the_original_line_on_top_and_edges_to_the_right() {
    let temp = TempDir::new().unwrap();
    let source = fixture(temp.path(), "(;SZ[9];B[cc];W[gg];B[cg])");
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let line = app.document().unwrap().mainline.clone();
    app.select(line[1]).unwrap();
    let branch = app.play(Some(Point::new(2, 3))).unwrap();
    app.play(Some(Point::new(3, 3))).unwrap();
    app.select(branch).unwrap();
    app.play(Some(Point::new(4, 4))).unwrap();
    let doc = app.document().unwrap();
    let layout = doc.layout();
    assert_eq!(layout.len(), doc.nodes.len());
    for id in &line {
        assert_eq!(layout.iter().find(|p| p.id == *id).unwrap().row, 0);
    }
    for p in &layout {
        assert_eq!(
            layout
                .iter()
                .filter(|other| other.column == p.column && other.row == p.row)
                .count(),
            1
        );
        if let Some(parent) = doc.nodes[p.id].parent {
            assert!(p.column > layout.iter().find(|q| q.id == parent).unwrap().column);
        }
    }
}

#[test]
fn invalid_import_does_not_destroy_the_current_review() {
    let temp = TempDir::new().unwrap();
    let source = fixture(temp.path(), "(;SZ[9];B[cc])");
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let before = serde_json::to_value(app.document()).unwrap();
    fs::write(&source, "this is not SGF").unwrap();
    assert!(app.import(&source).is_err());
    assert_eq!(serde_json::to_value(app.document()).unwrap(), before);
}
#[test]
fn handicap_setup_has_stable_identity_across_reopen_and_compressed_lists_expand() {
    let temp = TempDir::new().unwrap();
    let source = fixture(temp.path(), "(;SZ[9]HA[4]AB[cc][cg][gc][gg]PL[W])");
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let branch = app.play(Some(Point::new(4, 4))).unwrap();
    drop(app);
    for _ in 0..5 {
        let mut app = review(temp.path());
        app.import(&source).unwrap();
        assert_eq!(app.document().unwrap().nodes.len(), 2);
        assert_eq!(
            app.document()
                .unwrap()
                .board(branch)
                .unwrap()
                .stone(Point::new(4, 4)),
            Some(Color::White)
        );
    }
    let source = fixture(temp.path(), "(;SZ[9]AB[aa:bb]PL[W])");
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let board = app.document().unwrap().board(0).unwrap();
    assert_eq!(board.cells.iter().filter(|c| c.is_some()).count(), 4);
    assert_eq!(board.next, Color::White);
}
