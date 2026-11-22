use katastro::{
    Analysis, EngineProfile, Review,
    archive::{
        Game,
        categories::{Category, Thresholds, classify, evidence},
    },
};
use std::fs;
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    review: Review,
    game: Game,
    profile: EngineProfile,
}

impl Fixture {
    fn new(metadata: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("game.sgf");
        fs::write(
            &path,
            format!("(;SZ[9]{metadata};B[aa](;W[bb];B[cc])(;W[dc]))"),
        )
        .unwrap();
        let mut review =
            Review::new(&root.path().join("reviews"), &root.path().join("cache")).unwrap();
        review.import(&path).unwrap();
        let game = Game {
            id: "1".into(),
            date: "2026-01-01".into(),
            white: "White Player 2k".into(),
            black: "Black Player 1d".into(),
            result: "Black won by time".into(),
            path,
        };
        let profile = EngineProfile {
            model_sha256: "model".into(),
            engine_sha256: "engine".into(),
            settings_digest: "settings".into(),
        };
        Self {
            root,
            review,
            game,
            profile,
        }
    }
    fn score(&mut self, node: usize, points: f64, visits: u64) {
        let key = self.review.analysis_key(node, &self.profile).unwrap();
        self.review
            .store_analysis(
                &key,
                &Analysis {
                    visits,
                    score_lead: points,
                    winrate: 0.5,
                    ownership: vec![],
                    ownership_visits: 0,
                    suggestions: vec![],
                },
            )
            .unwrap();
    }
    fn category(&self, account: &str) -> Category {
        classify(&self.review, &self.game, account, Some(&self.profile)).unwrap()
    }
}

// A twenty-point improvement from a handicap-adjusted starting score qualifies
// on the eventual loser's side, in both colors, even with missing later positions.
#[test]
fn twenty_point_swing_identifies_both_accounts_from_the_sgf_result_and_players() {
    for (winner, swing, loser_name, winner_name) in [
        ("W+R", 20.0, "black player", "White Player"),
        ("B+0.5", -20.0, "White Player", "black player"),
    ] {
        let mut f = Fixture::new(&format!("PB[Black Player]PW[White Player]RE[{winner}]"));
        std::mem::swap(&mut f.game.white, &mut f.game.black); // old archive metadata disagrees
        f.score(0, 31.0, 64);
        f.score(1, 31.0 + swing, 64);
        assert_eq!(f.category(loser_name), Category::Blunder);
        assert_eq!(f.category(winner_name), Category::Comeback);
        assert_eq!(f.category("spectator"), Category::Unavailable);
    }
}

// Incomplete coverage cannot rule a swing out. A user's variation, an older
// score, or results from another model must not keep a game in the wrong filter.
#[test]
fn only_compatible_recorded_scores_count_and_deeper_scores_can_reclassify() {
    let mut f = Fixture::new("PB[Black Player]PW[White Player]RE[W+R]");
    f.score(2, 51.0, 64);
    assert_eq!(
        f.category("Black Player"),
        Category::Pending,
        "missing baseline"
    );
    f.score(0, 31.0, 64);
    assert_eq!(f.category("Black Player"), Category::Blunder);
    f.score(2, 50.99, 128);
    f.score(4, 70.0, 128); // SGF side variation
    assert_eq!(f.category("Black Player"), Category::Pending);
    f.score(1, 31.0, 64);
    f.score(3, -10.0, 64); // the winner gained ground instead
    let different_model = EngineProfile {
        model_sha256: "other-model".into(),
        ..f.profile.clone()
    };
    assert_eq!(
        classify(&f.review, &f.game, "Black Player", Some(&different_model)).unwrap(),
        Category::Pending
    );
    assert_eq!(
        classify(&f.review, &f.game, "Black Player", None).unwrap(),
        Category::Pending
    );
    let reopened =
        Review::new(&f.root.path().join("reviews"), &f.root.path().join("cache")).unwrap();
    assert_eq!(
        classify(&reopened, &f.game, "Black Player", Some(&f.profile)).unwrap(),
        Category::Other
    );
    assert!(
        reopened.document().is_none(),
        "library statistics must not open the review"
    );
}

// Archives may carry the only recorded outcome or names. Explicit draws,
// unfinished games and ambiguous owners cannot contribute to either statistic.
#[test]
fn indexed_names_ranks_and_pandanet_results_supply_missing_sgf_metadata() {
    let mut f = Fixture::new("");
    f.score(0, -7.5, 64);
    f.score(1, -28.0, 64);
    assert_eq!(f.category("Black Player"), Category::Comeback);
    assert_eq!(f.category("White Player"), Category::Blunder);
    f.game.black = "WHITE PLAYER 1d".into();
    assert_eq!(f.category("White Player"), Category::Unavailable);
    f.game.black = "Black Player 1d".into();
    for result in ["Draw", "Game unfinished", "Void", ""] {
        f.game.result = result.into();
        assert_eq!(f.category("Black Player"), Category::Unavailable);
    }
    let mut draw = Fixture::new("PB[Black Player]PW[White Player]RE[0]");
    draw.score(0, 5.0, 64);
    draw.score(1, -16.0, 64);
    assert_eq!(
        draw.category("Black Player"),
        Category::Unavailable,
        "explicit SGF draw outranks stale indexed result"
    );
}

// Both owners can apply their own cutoff to the same recorded swing. With
// incomplete coverage, raising the cutoff can put a game back into Pending.
#[test]
fn independent_thresholds_use_the_largest_observed_gain_and_keep_partial_games_pending() {
    let mut f = Fixture::new("PB[Black Player]PW[White Player]RE[W+T]");
    f.score(0, -9.0, 64);
    f.score(1, 12.0, 64);
    f.score(2, 23.5, 128);
    let cached =
        |f: &Fixture, player| evidence(&f.review, &f.game, player, Some(&f.profile)).unwrap();
    let win = cached(&f, "White Player");
    let loss = cached(&f, "Black Player");
    let high_blunder = Thresholds {
        comeback: 32.5,
        blunder: 33.0,
    };
    let high_comeback = Thresholds {
        comeback: 33.0,
        blunder: 32.5,
    };
    assert_eq!(win.category(high_blunder), Category::Comeback);
    assert_eq!(loss.category(high_blunder), Category::Pending);
    assert_eq!(win.category(high_comeback), Category::Pending);
    assert_eq!(loss.category(high_comeback), Category::Blunder);
    assert_eq!((win.won(), loss.won()), (Some(true), Some(false)));

    f.score(3, -16.0, 64);
    assert_eq!(
        cached(&f, "Black Player").category(high_blunder),
        Category::Other
    );
    assert_eq!(
        cached(&f, "White Player").category(high_blunder),
        Category::Comeback
    );
}
