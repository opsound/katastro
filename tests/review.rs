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

// Refactoring shared edit persistence must retain rollback on a failed save,
// for both navigation and branch creation, without touching the source SGF.
#[test]
fn failed_autosaves_leave_selection_and_variations_unchanged() {
    let temp = TempDir::new().unwrap();
    let source = fixture(temp.path(), "(;SZ[9];B[cc];W[gg])");
    let original = fs::read(&source).unwrap();
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let before = serde_json::to_value(app.document()).unwrap();
    let db = rusqlite::Connection::open(temp.path().join("reviews.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_save BEFORE UPDATE ON documents BEGIN SELECT RAISE(FAIL, 'storage unavailable'); END;").unwrap();
    assert!(app.select(1).is_err());
    assert_eq!(serde_json::to_value(app.document()).unwrap(), before);
    assert!(app.play(Some(Point::new(4, 4))).is_err());
    assert_eq!(serde_json::to_value(app.document()).unwrap(), before);
    db.execute_batch("DROP TRIGGER reject_save").unwrap();
    let mut reopened = review(temp.path());
    reopened.import(&source).unwrap();
    assert_eq!(serde_json::to_value(reopened.document()).unwrap(), before);
    let added = app.play(Some(Point::new(4, 4))).unwrap();
    reopened.import(&source).unwrap();
    assert_eq!(reopened.document().unwrap().selected, added);
    assert_eq!(reopened.document().unwrap().mainline, vec![0, 1, 2]);
    assert_eq!(fs::read(source).unwrap(), original);
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
fn compact_rows_reuse_finished_forks_and_keep_overlapping_continuations_traceable() {
    let temp = TempDir::new().unwrap();
    let source = fixture(temp.path(), "(;SZ[9];B[];W[];B[];W[];B[];W[])");
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let line = app.document().unwrap().mainline.clone();

    // Two adjacent one-move forks, then a two-move fork with two replies.
    app.select(line[1]).unwrap();
    let early = app.play(Some(Point::new(0, 0))).unwrap();
    app.select(line[2]).unwrap();
    let adjacent = app.play(Some(Point::new(1, 0))).unwrap();
    app.select(line[4]).unwrap();
    let fork = app.play(Some(Point::new(2, 0))).unwrap();
    let reply = app.play(Some(Point::new(2, 1))).unwrap();
    app.select(fork).unwrap();
    let alternative = app.play(Some(Point::new(3, 1))).unwrap();

    // This line begins beside the short fork but later overlaps both replies.
    app.select(line[2]).unwrap();
    let long = [
        app.play(Some(Point::new(4, 0))).unwrap(),
        app.play(None).unwrap(),
        app.play(None).unwrap(),
        app.play(Some(Point::new(4, 1))).unwrap(),
    ];
    let layout = app.document().unwrap().layout();
    for id in [early, adjacent, fork, reply] {
        assert_eq!(
            layout[id].row, 1,
            "disjoint forks share the first variation row"
        );
    }
    assert!(layout[alternative].row > layout[reply].row);
    for id in long {
        assert!(
            layout[id].row > layout[alternative].row,
            "overlapping continuations keep their order"
        );
        assert_eq!(
            layout[id].row, layout[long[0]].row,
            "a continuation stays horizontal"
        );
    }

    app.select(reply).unwrap();
    let neighbor = app.document().unwrap().vertical_neighbor(true).unwrap();
    assert_eq!(neighbor, alternative);
    app.select(neighbor).unwrap();
    let doc = app.document().unwrap();
    assert_eq!(
        doc.board(doc.selected).unwrap().stone(Point::new(3, 1)),
        Some(Color::White)
    );
    assert_eq!(doc.vertical_neighbor(true), Some(long[3]));
    let mut reopened = review(temp.path());
    reopened.import(&source).unwrap();
    assert_eq!(reopened.document().unwrap().layout(), layout);
}

#[test]
fn playing_beyond_the_imported_game_starts_a_variation_under_the_original_endpoint() {
    let temp = TempDir::new().unwrap();
    let source = fixture(temp.path(), "(;SZ[9];B[aa];W[bb])");
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let original = app.document().unwrap().mainline.clone();
    let endpoint = *original.last().unwrap();
    app.select(endpoint).unwrap();
    let extension = app.play(Some(Point::new(2, 2))).unwrap();
    let reply = app.play(None).unwrap();
    let doc = app.document().unwrap();
    let layout = doc.layout();
    assert_eq!(
        layout[extension].row, 1,
        "the played row ends at the imported endpoint"
    );
    assert_eq!(layout[reply].row, layout[extension].row);
    assert_eq!(layout[extension].column, layout[endpoint].column + 1);
    assert!(original.iter().all(|id| layout[*id].row == 0));
    assert_eq!(doc.continuation(endpoint), Some(extension));
}

#[test]
fn tree_move_numbers_follow_the_board_across_root_moves_passes_and_nonmove_nodes() {
    let temp = TempDir::new().unwrap();
    let source = fixture(temp.path(), "(;SZ[9]B[aa];C[diagram];AW[bb];W[];B[cc])");
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    app.select(2).unwrap();
    let alternative = app.play(Some(Point::new(3, 3))).unwrap();
    let doc = app.document().unwrap();
    let layout = doc.layout();
    for id in doc.mainline.iter().copied().chain([alternative]) {
        assert_eq!(
            layout[id].move_number,
            doc.board(id).unwrap().move_number,
            "tree move number at node {id}"
        );
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
fn import_validates_every_branch_with_independent_board_and_ko_history() {
    let temp = TempDir::new().unwrap();
    let source = fixture(
        temp.path(),
        "(;SZ[5]RU[Japanese]AB[ab][ba][cb]AW[bb][ac][cc][bd]PL[B](;B[bc];W[];B[];W[bb])(;B[bc];W[ee]))",
    );
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let last = *app.document().unwrap().mainline.last().unwrap();
    app.select(last).unwrap();
    assert_eq!(
        app.document()
            .unwrap()
            .board(last)
            .unwrap()
            .stone(Point::new(1, 1)),
        Some(Color::White)
    );
    let saved = serde_json::to_value(app.document()).unwrap();
    // The illegal recapture is in a sibling, after a legal first branch.
    fs::write(&source, "(;SZ[5]RU[Japanese]AB[ab][ba][cb]AW[bb][ac][cc][bd]PL[B](;B[bc];W[];B[];W[bb])(;B[bc];W[bb]))").unwrap();
    assert!(app.import(&source).unwrap_err().to_string().contains("Ko"));
    assert_eq!(serde_json::to_value(app.document()).unwrap(), saved);
    // A setup edit resets history on its own path and must not leak to siblings.
    fs::write(&source, "(;SZ[5];B[aa](;AE[aa]PL[B];B[aa])(;W[bb]))").unwrap();
    app.import(&source).unwrap();
    assert_eq!(
        app.document()
            .unwrap()
            .board(4)
            .unwrap()
            .stone(Point::new(0, 0)),
        Some(Color::Black)
    );
    assert!(app.document().unwrap().position(3).is_err());
    app.select(4).unwrap();
    let branch = app.play(Some(Point::new(2, 2))).unwrap();
    drop(app);
    let mut reopened = review(temp.path());
    reopened.import(&source).unwrap();
    assert_eq!(reopened.document().unwrap().selected, branch);
    assert_eq!(
        reopened
            .document()
            .unwrap()
            .board(branch)
            .unwrap()
            .stone(Point::new(2, 2)),
        Some(Color::Black)
    );
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

#[cfg(unix)]
#[test]
fn exporting_to_a_source_alias_preserves_the_original_sgf() {
    let temp = TempDir::new().unwrap();
    let source = fixture(temp.path(), "(;SZ[9];B[cc])");
    let original = fs::read(&source).unwrap();
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let branch = app.play(Some(Point::new(3, 3))).unwrap();
    let hard = temp.path().join("hard-link.sgf");
    let symbolic = temp.path().join("symbolic-link.sgf");
    fs::hard_link(&source, &hard).unwrap();
    std::os::unix::fs::symlink(&source, &symbolic).unwrap();
    for alias in [hard, symbolic, source.clone()] {
        let result = app.export(&alias);
        assert_eq!(
            fs::read(&source).unwrap(),
            original,
            "exporting to an alias must not overwrite the original SGF"
        );
        assert!(result.is_err(), "export must reject aliases of its source");
    }
    let exported = temp.path().join("review.sgf");
    app.export(&exported).unwrap();
    let mut reopened = review(&temp.path().join("exported"));
    reopened.import(&exported).unwrap();
    let doc = reopened.document().unwrap();
    assert_eq!(doc.nodes.len(), 3);
    assert_eq!(
        doc.board(branch).unwrap().stone(Point::new(3, 3)),
        Some(Color::Black)
    );
    assert_eq!(fs::read(&source).unwrap(), original);
}

#[test]
fn new_zealand_superko_distinguishes_the_next_player_and_persists_legal_suicide() {
    let temp = TempDir::new().unwrap();
    let source = fixture(
        temp.path(),
        "(;SZ[5]RU[New Zealand]AW[ab][ba][bc][ca][cc][db]PL[B];B[bb];W[])",
    );
    let mut app = review(temp.path());
    app.import(&source).unwrap();
    let mainline = app.document().unwrap().mainline.clone();
    app.select(2).unwrap();
    // Removing this two-stone chain restores the root's stones, but White is
    // now to play. NZ uses situational superko, so this position is legal.
    let removed = app
        .play(Some(Point::new(2, 1)))
        .expect("NZ permits a repeated stone arrangement with a different next player");
    let board = app.document().unwrap().board(removed).unwrap();
    assert_eq!(board.cells, app.document().unwrap().board(0).unwrap().cells);
    assert_eq!(board.next, Color::White);
    assert_eq!(board.captures, [0, 2]);
    let pass = app.play(None).unwrap();
    let before = serde_json::to_value(app.document()).unwrap();
    assert!(
        app.play(Some(Point::new(1, 1))).is_err(),
        "repeating both the stones and the next player must still be rejected"
    );
    assert_eq!(serde_json::to_value(app.document()).unwrap(), before);
    drop(app);
    let mut reopened = review(temp.path());
    reopened.import(&source).unwrap();
    let doc = reopened.document().unwrap();
    assert_eq!(doc.mainline, mainline);
    assert_eq!(doc.selected, pass);
    assert_eq!(doc.board(removed).unwrap().captures, [0, 2]);
    assert_eq!(doc.board(pass).unwrap().next, Color::Black);
}

#[test]
fn simultaneous_reviews_preserve_branches_and_each_sessions_move_parent() {
    let temp = TempDir::new().unwrap();
    let sgf = "(;SZ[9];B[aa];W[bb])";
    let source = fixture(temp.path(), sgf);
    let mut first = review(temp.path());
    let mut second = review(temp.path());
    first.import(&source).unwrap();
    second.import(&source).unwrap();

    first.select(1).unwrap();
    let branch = first.play(Some(Point::new(3, 3))).unwrap();
    second.select(2).unwrap();
    let continuation = second.play(Some(Point::new(4, 4))).unwrap();
    let reply = first.play(Some(Point::new(5, 5))).unwrap();

    let mut reopened = review(temp.path());
    reopened.import(&source).unwrap();
    let doc = reopened.document().unwrap();
    assert_eq!(
        doc.nodes.len(),
        6,
        "an autosave discarded another session's move"
    );
    assert_eq!(doc.nodes[branch].parent, Some(1));
    assert_eq!(doc.nodes[continuation].parent, Some(2));
    assert_eq!(doc.nodes[reply].parent, Some(branch));
    assert_eq!(doc.selected, reply);
    assert_eq!(doc.mainline, vec![0, 1, 2]);
    assert_eq!(fs::read_to_string(source).unwrap(), sgf);
}
