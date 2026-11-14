//! KataGo result decoding, shared by review scheduling and training.
use super::{Analysis, SuggestedMove};
use crate::{Point, Result};
use serde_json::Value;
impl Analysis {
    pub(crate) fn from_root(reply: &Value) -> Result<Self> {
        let root = reply
            .get("rootInfo")
            .ok_or("Engine reply has no evaluation")?;
        let result = Self {
            visits: root["visits"].as_u64().ok_or("Invalid visit count")?,
            winrate: root["winrate"].as_f64().ok_or("Invalid winrate")?,
            score_lead: root["scoreLead"].as_f64().ok_or("Invalid score")?,
            suggestions: vec![],
            ownership: vec![],
            ownership_visits: 0,
        };
        result.validate()?;
        Ok(result)
    }
    pub(crate) fn from_reply(reply: &Value, size: usize) -> Result<Self> {
        let mut result = Self::from_root(reply)?;
        result.suggestions = parse_suggestions(reply, size)?;
        if let Some(value) = reply.get("ownership") {
            result.ownership = serde_json::from_value(value.clone())?;
            if result.ownership.len() != size * size {
                return Err("Ownership does not match the board size".into());
            }
            result.ownership_visits = result.visits;
        }
        result.validate()?;
        Ok(result)
    }
}
fn parse_suggestions(reply: &Value, size: usize) -> Result<Vec<SuggestedMove>> {
    let moves = reply.get("moveInfos").and_then(Value::as_array);
    if moves.is_none_or(|moves| moves.is_empty()) {
        let Some(policy) = reply.get("policy").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };
        if policy.len() != size * size + 1 {
            return Err("Invalid policy dimensions".into());
        }
        let mut ranked: Vec<_> = policy
            .iter()
            .enumerate()
            .filter_map(|(i, value)| {
                value
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .map(|p| (i, p))
            })
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        return Ok(ranked
            .into_iter()
            .map(|(i, _)| SuggestedMove {
                point: if i == size * size {
                    None
                } else {
                    Some(Point::new(i % size, i / size))
                },
                visits: 0,
                winrate: None,
                score_lead: None,
            })
            .collect());
    }
    let moves = moves.unwrap();
    let mut ranked: Vec<_> = moves.iter().collect();
    ranked.sort_by_key(|m| m["order"].as_u64().unwrap_or(u64::MAX));
    ranked
        .into_iter()
        .map(|m| {
            Ok(SuggestedMove {
                point: Point::from_gtp(
                    m["move"]
                        .as_str()
                        .ok_or("Suggested move has no coordinate")?,
                    size,
                )?,
                visits: m["visits"]
                    .as_u64()
                    .ok_or("Suggested move has no visit count")?,
                winrate: Some(
                    m["winrate"]
                        .as_f64()
                        .ok_or("Suggested move has no winrate")?,
                ),
                score_lead: Some(
                    m["scoreLead"]
                        .as_f64()
                        .ok_or("Suggested move has no score")?,
                ),
            })
        })
        .collect()
}
