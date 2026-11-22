//! Classify the synced player's recorded games using durable KataGo point estimates.
use super::{Game, rank_history};
use crate::{Color, Document, EngineProfile, Result, Review, analysis::MIN_CACHE_VISITS};
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Comeback,
    Blunder,
    Other,
    Pending,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Thresholds {
    pub comeback: f64,
    pub blunder: f64,
}
impl Default for Thresholds {
    fn default() -> Self {
        Self {
            comeback: 20.0,
            blunder: 20.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Evidence {
    #[default]
    Pending,
    Unavailable,
    Unscored {
        won: bool,
    },
    Scored {
        won: bool,
        largest_gain: f64,
        complete: bool,
    },
}
impl Evidence {
    pub fn category(self, thresholds: Thresholds) -> Category {
        match self {
            Self::Scored {
                won: true,
                largest_gain,
                ..
            } if largest_gain >= thresholds.comeback => Category::Comeback,
            Self::Scored {
                won: false,
                largest_gain,
                ..
            } if largest_gain >= thresholds.blunder => Category::Blunder,
            Self::Scored { complete: true, .. } => Category::Other,
            Self::Unavailable => Category::Unavailable,
            _ => Category::Pending,
        }
    }
    pub fn won(self) -> Option<bool> {
        match self {
            Self::Unscored { won } | Self::Scored { won, .. } => Some(won),
            _ => None,
        }
    }
}

pub(crate) fn winner_and_reason(result: &str) -> Option<(Color, &str)> {
    let value = result.trim();
    for (prefix, color) in [
        ("B+", Color::Black),
        ("W+", Color::White),
        ("Black won by ", Color::Black),
        ("White won by ", Color::White),
    ] {
        if let Some((start, reason)) = value.split_at_checked(prefix.len())
            && start.eq_ignore_ascii_case(prefix)
        {
            return Some((color, reason.trim()));
        }
    }
    None
}

pub fn classify(
    review: &Review,
    game: &Game,
    account: &str,
    profile: Option<&EngineProfile>,
) -> Result<Category> {
    Ok(evidence(review, game, account, profile)?.category(Thresholds::default()))
}

/// Read the recorded loser's greatest gain once so each threshold can be applied in memory.
pub fn evidence(
    review: &Review,
    game: &Game,
    account: &str,
    profile: Option<&EngineProfile>,
) -> Result<Evidence> {
    let account = account.trim().to_lowercase();
    if account.is_empty() {
        return Ok(Evidence::Unavailable);
    }
    let doc = Document::parse(&std::fs::read(&game.path)?)?;
    let root = &doc.nodes[0];
    let property = |name| root.property(name).map(str::trim).filter(|s| !s.is_empty());
    let Some((winner, _)) = winner_and_reason(property("RE").unwrap_or(&game.result)) else {
        return Ok(Evidence::Unavailable);
    };

    let owns = |name, indexed: &str| {
        if let Some(name) = property(name) {
            name.to_lowercase() == account
        } else {
            indexed.trim().to_lowercase() == account
                || rank_history::player(indexed).0.to_lowercase() == account
        }
    };
    let owner = match (owns("PB", &game.black), owns("PW", &game.white)) {
        (true, false) => Color::Black,
        (false, true) => Color::White,
        _ => return Ok(Evidence::Unavailable),
    };
    let won = winner == owner;
    let Some(profile) = profile else {
        return Ok(Evidence::Unscored { won });
    };
    // One read transaction keeps the starting score and the rest of the recorded
    // line consistent when a worker refines positions during the scan.
    let cache = review.cache.unchecked_transaction()?;
    let mut scores = cache.prepare_cached(
        "SELECT json_extract(result, '$.score_lead') FROM analysis WHERE key=?1 AND visits>=?2",
    )?;
    let mut score = |node| -> Result<Option<f64>> {
        let Ok(position) = doc.position(node) else {
            return Ok(None);
        };
        Ok(scores
            .query_row(
                params![position.analysis_key(profile)?, MIN_CACHE_VISITS as i64],
                |r| r.get(0),
            )
            .optional()?)
    };
    let Some(start) = score(doc.mainline[0])? else {
        return Ok(Evidence::Unscored { won });
    };
    let mut complete = true;
    let mut largest_gain: f64 = 0.0;
    for &node in &doc.mainline[1..] {
        match score(node)? {
            Some(value) => {
                largest_gain = largest_gain.max(winner.other().perspective(value - start))
            }
            None => complete = false,
        }
    }
    Ok(Evidence::Scored {
        won,
        largest_gain,
        complete,
    })
}
