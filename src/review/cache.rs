//! Durable analysis components; user variations stay in the review database.
use super::*;
use crate::{Analysis, EngineProfile, analysis::MIN_CACHE_VISITS};
use std::collections::BTreeMap;
fn read_analysis(db: &Connection, key: &str, visits: u64) -> Result<Option<Analysis>> {
    let value: Option<String> = db
        .prepare_cached("SELECT result FROM analysis WHERE key=?1 AND visits>=?2")?
        .query_row(params![key, i64::try_from(visits)?], |row| row.get(0))
        .optional()?;
    value
        .map(|json| {
            let value: Analysis = serde_json::from_str(&json)?;
            value.validate()?;
            Ok(value)
        })
        .transpose()
}
impl Review {
    pub fn analysis_key(&self, node: NodeId, profile: &EngineProfile) -> Result<String> {
        let position = self
            .document
            .as_ref()
            .ok_or("Open an SGF first")?
            .position(node)?;
        position.analysis_key(profile)
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
        let combined = if let Some(old) = read_analysis(&transaction, key, 0)? {
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
            for id in 0..doc.nodes.len() {
                let Ok(key) = self.analysis_key(id, profile) else {
                    continue;
                };
                if let Some(result) = self.analysis(&key, 0)? {
                    results.insert(id, result);
                }
            }
        }
        Ok(results)
    }
    pub(crate) fn analysis(&self, key: &str, visits: u64) -> Result<Option<Analysis>> {
        read_analysis(&self.cache, key, visits)
    }
    pub fn clear_analysis(&mut self) -> Result<()> {
        self.cache
            .execute_batch("BEGIN; DELETE FROM analysis; DELETE FROM training_games; COMMIT;")?;
        Ok(())
    }
    pub fn save_profile(&self, profile: &EngineProfile) -> Result<()> {
        self.save_setting("profile", profile)
    }
    pub fn last_profile(&self) -> Result<Option<EngineProfile>> {
        self.setting("profile")
    }
}
