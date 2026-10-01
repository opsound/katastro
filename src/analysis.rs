use crate::{Document, NodeId, Result, Review, digest};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const MIN_CACHE_VISITS: u64 = 64;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SuggestedMove {
    pub point: Option<crate::Point>,
    pub visits: u64,
    pub winrate: Option<f64>,
    pub score_lead: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub visits: u64,
    pub winrate: f64,
    pub score_lead: f64,
    #[serde(default)]
    pub suggestions: Vec<SuggestedMove>,
    /// Row-major expected ownership, from Black's perspective. Empty in older results.
    #[serde(default)]
    pub ownership: Vec<f64>,
    /// Map effort can differ from chart effort; zero infers the depth of a legacy map.
    #[serde(default)]
    pub ownership_visits: u64,
}
impl Analysis {
    pub fn ownership_depth(&self) -> u64 {
        if self.ownership.is_empty() {
            0
        } else if self.ownership_visits == 0 {
            self.visits
        } else {
            self.ownership_visits
        }
    }
    pub(crate) fn merge(&self, incoming: &Self) -> Self {
        let mut result = if incoming.visits >= self.visits {
            incoming.clone()
        } else {
            self.clone()
        };
        let ownership = if incoming.ownership_depth() >= self.ownership_depth() {
            incoming
        } else {
            self
        };
        result.ownership = ownership.ownership.clone();
        result.ownership_visits = ownership.ownership_depth();
        result
    }
    pub fn validate(&self) -> Result<()> {
        if self.visits == 0
            || !self.winrate.is_finite()
            || !(0.0..=1.0).contains(&self.winrate)
            || !self.score_lead.is_finite()
        {
            return Err("Invalid analysis result".into());
        }
        for suggestion in &self.suggestions {
            if suggestion
                .winrate
                .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
                || suggestion.score_lead.is_some_and(|v| !v.is_finite())
            {
                return Err("Invalid suggested move evaluation".into());
            }
        }
        if self
            .ownership
            .iter()
            .any(|v| !v.is_finite() || !(-1.0..=1.0).contains(v))
        {
            return Err("Invalid ownership estimate".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineProfile {
    pub model_sha256: String,
    pub engine_sha256: String,
    pub settings_digest: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub board_x_size: usize,
    pub board_y_size: usize,
    pub rules: String,
    pub komi: f64,
    pub initial_stones: Vec<(String, String)>,
    pub initial_player: String,
    pub moves: Vec<(String, String)>,
}
impl Document {
    pub fn position(&self, node: NodeId) -> Result<Position> {
        let path = self.path(node)?;
        if path.iter().skip(1).any(|id| {
            ["AB", "AW", "AE", "PL"]
                .iter()
                .any(|p| self.nodes[*id].props.contains_key(*p))
        }) {
            return Err("Analysis of setup edits after the root is not supported; review and export remain available".into());
        }
        let mut root = self.clone();
        root.nodes[0].played = None;
        let initial = root.board(0)?;
        let initial_stones = initial
            .cells
            .iter()
            .enumerate()
            .filter_map(|(i, color)| {
                color.map(|color| {
                    (
                        color.code().into(),
                        crate::Point::new(i % self.size, i / self.size).gtp(self.size),
                    )
                })
            })
            .collect();
        let moves = path
            .iter()
            .filter_map(|id| self.nodes[*id].played)
            .map(|m| {
                (
                    m.color.code().into(),
                    m.point.map(|p| p.gtp(self.size)).unwrap_or("pass".into()),
                )
            })
            .collect();
        Ok(Position {
            board_x_size: self.size,
            board_y_size: self.size,
            rules: self.rules.clone(),
            komi: self.komi,
            initial_stones,
            initial_player: initial.next.code().into(),
            moves,
        })
    }
    pub fn source_identity(&self) -> Result<String> {
        let nodes: Vec<_> = self
            .nodes
            .iter()
            .map(|node| {
                let props: BTreeMap<_, _> = node
                    .props
                    .iter()
                    .filter(|(key, _)| matches!(key.as_str(), "AB" | "AW" | "AE" | "PL" | "HA"))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                (node.parent, node.children.clone(), node.played, props)
            })
            .collect();
        Ok(digest(&serde_json::to_vec(&(
            self.size,
            &self.rules,
            self.komi,
            nodes,
        ))?))
    }
}
impl Review {
    pub fn analysis_key(&self, node: NodeId, profile: &EngineProfile) -> Result<String> {
        let position = self
            .document
            .as_ref()
            .ok_or("Open an SGF first")?
            .position(node)?;
        Ok(digest(&serde_json::to_vec(&(1, profile, position))?))
    }
    pub fn store_analysis(&mut self, key: &str, analysis: &Analysis) -> Result<()> {
        analysis.validate()?;
        if analysis.visits < MIN_CACHE_VISITS {
            return Ok(());
        }
        let mut eligible = analysis.clone();
        if eligible.ownership_depth() < MIN_CACHE_VISITS {
            eligible.ownership.clear();
            eligible.ownership_visits = 0;
        }
        // Merge completed components under a write lock, including when another app
        // or review is updating the same cache. A quick map cannot downgrade a chart.
        let transaction = self
            .cache
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let old: Option<String> = transaction
            .query_row("SELECT result FROM analysis WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        let combined = if let Some(old) = old {
            let old: Analysis = serde_json::from_str(&old)?;
            old.validate()?;
            old.merge(&eligible)
        } else {
            eligible
        };
        transaction.execute("INSERT INTO analysis(key,visits,result) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET visits=excluded.visits,result=excluded.result WHERE excluded.visits >= analysis.visits",params![key,i64::try_from(combined.visits)?,serde_json::to_string(&combined)?])?;
        transaction.commit()?;
        Ok(())
    }
    pub fn cached_analysis(&self, profile: &EngineProfile) -> Result<BTreeMap<NodeId, Analysis>> {
        let mut results = BTreeMap::new();
        if let Some(doc) = &self.document {
            let mut stmt = self
                .cache
                .prepare("SELECT result FROM analysis WHERE key=?1")?;
            for id in 0..doc.nodes.len() {
                let Ok(key) = self.analysis_key(id, profile) else {
                    continue;
                };
                let result: Option<String> = stmt.query_row([key], |row| row.get(0)).optional()?;
                if let Some(result) = result {
                    let result: Analysis = serde_json::from_str(&result)?;
                    result.validate()?;
                    results.insert(id, result);
                }
            }
        }
        Ok(results)
    }
    pub fn clear_analysis(&mut self) -> Result<()> {
        self.cache.execute("DELETE FROM analysis", [])?;
        Ok(())
    }
    pub fn save_profile(&self, profile: &EngineProfile) -> Result<()> {
        self.reviews.execute("INSERT INTO settings(key,value) VALUES ('profile',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(profile)?])?;
        Ok(())
    }
    pub fn last_profile(&self) -> Result<Option<EngineProfile>> {
        let value: Option<String> = self
            .reviews
            .query_row("SELECT value FROM settings WHERE key='profile'", [], |r| {
                r.get(0)
            })
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .transpose()
    }
}
