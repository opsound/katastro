use crate::{Analysis, Document, EngineProfile, NodeId, Point, Result, SuggestedMove, digest};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub struct Target {
    pub node: NodeId,
    pub key: String,
}
#[derive(Clone, Debug)]
pub struct Request {
    pub id: String,
    pub query: Value,
    pub targets: BTreeMap<usize, Vec<Target>>,
    pub visits: u64,
}
pub struct Scheduler {
    pub values: BTreeMap<NodeId, Analysis>,
    doc: Document,
    profile: EngineProfile,
    generation: u64,
    sequence: u64,
    inflight: BTreeMap<String, Request>,
    failed: BTreeSet<NodeId>,
    interactive: Option<NodeId>,
}
impl Scheduler {
    pub fn new(
        generation: u64,
        doc: Document,
        profile: EngineProfile,
        values: BTreeMap<NodeId, Analysis>,
    ) -> Self {
        let interactive = values
            .get(&doc.selected)
            .filter(|v| v.visits >= 64)
            .map(|_| doc.selected);
        Self {
            values,
            doc,
            profile,
            generation,
            sequence: 0,
            inflight: BTreeMap::new(),
            failed: BTreeSet::new(),
            interactive,
        }
    }
    fn request(&mut self, nodes: &[NodeId], visits: u64, priority: i64) -> Result<Request> {
        let last = *nodes.last().ok_or("Empty analysis request")?;
        let position = self.doc.position(last)?;
        let mut targets: BTreeMap<usize, Vec<Target>> = BTreeMap::new();
        for node in nodes {
            let pos = self.doc.position(*node)?;
            let key = digest(&serde_json::to_vec(&(1, &self.profile, &pos))?);
            targets
                .entry(pos.moves.len())
                .or_default()
                .push(Target { node: *node, key });
        }
        self.sequence += 1;
        let id = format!("{}-{}", self.generation, self.sequence);
        let mut query = serde_json::to_value(position)?;
        let object = query.as_object_mut().ok_or("Invalid position")?;
        object.insert("id".into(), json!(id));
        object.insert("maxVisits".into(), json!(visits));
        object.insert(
            "analyzeTurns".into(),
            json!(targets.keys().copied().collect::<Vec<_>>()),
        );
        object.insert("priority".into(), json!(priority));
        object.insert("includeOwnership".into(), json!(visits > 1 || priority > 0));
        object.insert("includePolicy".into(), json!(visits == 1));
        object.insert("analysisPVLen".into(), json!(1));
        if visits > 1 {
            object.insert("reportDuringSearchEvery".into(), json!(0.2));
        }
        let request = Request {
            id,
            query,
            targets,
            visits,
        };
        self.inflight.insert(request.id.clone(), request.clone());
        Ok(request)
    }
    pub fn next_request(&mut self) -> Result<Option<Request>> {
        if self.inflight.len() >= 2 {
            return Ok(None);
        }
        if let Some(node) = self.interactive.take() {
            let current = self.values.get(&node).map_or(0, |v| v.visits);
            let missing_ownership = self
                .values
                .get(&node)
                .is_none_or(|v| v.ownership.is_empty());
            if !self.failed.contains(&node)
                && !self.inflight.values().any(|r| {
                    r.query["includeOwnership"] == true
                        && r.targets.values().flatten().any(|t| t.node == node)
                })
            {
                return self
                    .request(
                        &[node],
                        if current == 0 {
                            1
                        } else if current < 64 || missing_ownership {
                            64
                        } else {
                            budgets().find(|v| *v > current).unwrap_or(i64::MAX as u64)
                        },
                        100,
                    )
                    .map(Some);
            }
        }
        if !self.inflight.is_empty() {
            return Ok(None);
        }
        let missing: Vec<_> = self
            .doc
            .mainline
            .iter()
            .copied()
            .filter(|id| !self.values.contains_key(id) && !self.failed.contains(id))
            .collect();
        if !missing.is_empty() {
            return self.request(&missing, 1, 0).map(Some);
        }
        for visits in budgets() {
            let nodes: Vec<_> = self
                .doc
                .mainline
                .iter()
                .copied()
                .filter(|id| {
                    !self.failed.contains(id)
                        && self.values.get(id).map_or(0, |v| v.visits) < visits
                })
                .take(8)
                .collect();
            if !nodes.is_empty() {
                return self.request(&nodes, visits, -10).map(Some);
            }
            if let Some(node) = (0..self.doc.nodes.len()).find(|id| {
                !self.doc.mainline.contains(id)
                    && !self.failed.contains(id)
                    && self.values.get(id).map_or(0, |v| v.visits) < visits
            }) {
                // A sibling branch must have its own history in the engine query.
                return self.request(&[node], visits, -10).map(Some);
            }
        }
        Ok(None)
    }
    pub fn accept(&mut self, reply: &Value) -> Result<Vec<(Target, Analysis)>> {
        let Some(id) = reply.get("id").and_then(Value::as_str) else {
            return Err("Engine reply has no request ID".into());
        };
        if reply.get("action").is_some() || reply.get("warning").is_some() {
            return Ok(Vec::new());
        }
        let Some(request) = self.inflight.get_mut(id) else {
            return Ok(Vec::new());
        };
        if let Some(error) = reply.get("error") {
            for target in request.targets.values().flatten() {
                self.failed.insert(target.node);
            }
            self.inflight.remove(id);
            return Err(format!("KataGo: {error}").into());
        }
        let turn = reply
            .get("turnNumber")
            .and_then(Value::as_u64)
            .ok_or("Engine reply has no turn number")? as usize;
        let Some(targets) = request.targets.get(&turn).cloned() else {
            return Ok(Vec::new());
        };
        let final_reply = reply.get("isDuringSearch").and_then(Value::as_bool) == Some(false);
        let result = if reply.get("noResults").and_then(Value::as_bool) == Some(true) {
            None
        } else {
            let root = reply
                .get("rootInfo")
                .ok_or("Engine reply has no evaluation")?;
            let ownership = if let Some(value) = reply.get("ownership") {
                let ownership: Vec<f64> = serde_json::from_value(value.clone())?;
                if ownership.len() != self.doc.size * self.doc.size {
                    return Err("Ownership does not match the board size".into());
                }
                ownership
            } else {
                Vec::new()
            };
            let analysis = Analysis {
                ownership_visits: if ownership.is_empty() {
                    0
                } else {
                    root["visits"].as_u64().ok_or("Invalid visit count")?
                },
                ownership,
                visits: root["visits"].as_u64().ok_or("Invalid visit count")?,
                winrate: root["winrate"].as_f64().ok_or("Invalid winrate")?,
                score_lead: root["scoreLead"].as_f64().ok_or("Invalid score")?,
                suggestions: parse_suggestions(reply, self.doc.size)?,
            };
            analysis.validate()?;
            Some(analysis)
        };
        let mut accepted = Vec::new();
        if let Some(value) = result {
            for target in &targets {
                let old = self.values.get(&target.node);
                let persistable_response =
                    old.is_none_or(|old| value.visits >= old.visits) || !value.ownership.is_empty();
                let combined = old.map_or_else(|| value.clone(), |old| old.merge(&value));
                self.values.insert(target.node, combined);
                if persistable_response {
                    // Return the actual response for final-result caching. The display
                    // can contain earlier streamed components that are not complete.
                    accepted.push((target.clone(), value.clone()));
                }
            }
        } else if final_reply {
            for target in &targets {
                self.failed.insert(target.node);
            }
        }
        if final_reply {
            if (!self.doc.mainline.contains(&self.doc.selected) || request.query["priority"] == 100)
                && targets.iter().any(|t| t.node == self.doc.selected)
                && self
                    .values
                    .get(&self.doc.selected)
                    .is_some_and(|v| v.visits < 64)
            {
                self.interactive = Some(self.doc.selected);
            }
            request.targets.remove(&turn);
            if request.targets.is_empty() {
                self.inflight.remove(id);
            }
        }
        Ok(accepted)
    }
    pub fn select(&mut self, doc: Document) -> Vec<String> {
        let canceled: Vec<_> = self
            .inflight
            .values()
            // Keep the fast original-game chart pass, but supersede interactive
            // work even at one visit so it cannot occupy the new selection's slot.
            .filter(|r| r.visits > 1 || r.query["priority"] == 100)
            .map(|r| r.id.clone())
            .collect();
        for id in &canceled {
            self.inflight.remove(id);
        }
        self.interactive = Some(doc.selected);
        self.doc = doc;
        canceled
    }
    pub fn pending(&self) -> usize {
        self.inflight.len()
    }
    pub fn request_ids(&self) -> Vec<String> {
        self.inflight.keys().cloned().collect()
    }
    pub fn coverage(&self) -> (usize, usize) {
        (
            self.doc
                .mainline
                .iter()
                .filter(|id| self.values.contains_key(id))
                .count(),
            self.doc.mainline.len(),
        )
    }
    pub fn is_complete(&self) -> bool {
        self.inflight.is_empty()
            && self.interactive.is_none()
            && (0..self.doc.nodes.len()).all(|id| {
                self.failed.contains(&id)
                    || self
                        .values
                        .get(&id)
                        .is_some_and(|v| v.visits >= i64::MAX as u64)
            })
    }
    pub fn unsupported(&mut self) {
        for id in 0..self.doc.nodes.len() {
            if self.doc.position(id).is_err() {
                self.failed.insert(id);
            }
        }
    }
}

fn budgets() -> impl Iterator<Item = u64> {
    [1, 8]
        .into_iter()
        .chain(std::iter::successors(Some(64u64), |v| {
            v.checked_mul(4).filter(|v| *v <= i64::MAX as u64)
        }))
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
