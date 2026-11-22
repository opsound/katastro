//! Budgeted evaluation caching and request priority.
use super::*;
use crate::analysis::Position;
impl Trainer {
    pub fn next_request(&mut self) -> Result<Option<Value>> {
        // Grade readiness changes on commands/replies, not each worker poll.
        if self.state.phase == Phase::Searching {
            self.advance()?;
        }
        // Keep one analysis slot available for the next interactive HumanSL reply.
        let reserve = self.state.phase == Phase::YourTurn;
        if self.pending.len() >= self.parallel_positions - usize::from(reserve) {
            return Ok(None);
        }
        Ok(self.queue.pop_front().map(|job| {
            let q = job.query.clone();
            self.pending.insert(q["id"].as_str().unwrap().into(), job);
            q
        }))
    }
    pub(super) fn query(&mut self, doc: &Document, node: usize, visits: u64) -> Result<Value> {
        self.position_query(&doc.position(node)?, visits)
    }
    fn position_query(&mut self, position: &Position, visits: u64) -> Result<Value> {
        let mut q = serde_json::to_value(position)?;
        self.sequence += 1;
        q["id"] = json!(format!("training-{}-{}", self.instance, self.sequence));
        q["maxVisits"] = json!(visits);
        q["analyzeTurns"] = json!([q["moves"].as_array().unwrap().len()]);
        Ok(q)
    }
    pub(super) fn keys(
        &self,
        doc: &Document,
        node: usize,
        visits: u64,
    ) -> Result<(String, String)> {
        self.position_keys(&doc.position(node)?, visits)
    }
    fn position_keys(&self, position: &Position, visits: u64) -> Result<(String, String)> {
        let position = position.analysis_key(&self.profile)?;
        Ok((
            digest(&serde_json::to_vec(&("training-v1", &position, visits))?),
            position,
        ))
    }
    pub(super) fn need(
        &mut self,
        doc: &Document,
        node: usize,
        visits: u64,
        reuse_deeper: bool,
    ) -> Result<()> {
        self.need_position(&doc.position(node)?, visits, reuse_deeper)
    }
    pub(super) fn need_position(
        &mut self,
        request: &Position,
        visits: u64,
        reuse_deeper: bool,
    ) -> Result<()> {
        let (key, position) = self.position_keys(request, visits)?;
        if self.memo.contains_key(&key)
            || self
                .queue
                .iter()
                .chain(self.pending.values())
                .any(|j| matches!(&j.kind, Kind::Evaluation {key:k,..} if *k == key))
        {
            return Ok(());
        }
        let ordered = if reuse_deeper {
            [&position, &key]
        } else {
            [&key, &position]
        };
        for &cached_key in &ordered {
            if let Some(value) = self.review.analysis(cached_key, visits)? {
                if reuse_deeper
                    || cached_key == &key
                    || visits == SCREEN_VISITS
                    || value.visits == visits
                {
                    // The review key was missing or too shallow. Restore it from the
                    // compatible training entry so library statistics can use it too.
                    if reuse_deeper && cached_key == &key {
                        self.review.store_analysis(&position, &value)?;
                    }
                    self.memo.insert(key, value);
                    return Ok(());
                }
                break;
            }
        }
        let query = self.position_query(request, visits)?;
        self.queue.push_back(Job {
            query,
            kind: Kind::Evaluation {
                key,
                position,
                visits,
            },
        });
        Ok(())
    }
    pub(super) fn need_candidate(&mut self, doc: &Document, index: usize) -> Result<()> {
        let node = doc.mainline[index];
        self.need(doc, node, GRADE_VISITS, self.preparing)?;
        let (key, position) = self.keys(doc, node, GRADE_VISITS)?;
        if self
            .memo
            .get(&key)
            .is_some_and(|a| !a.suggestions.iter().any(|m| m.visits > 0))
        {
            if let Some(cached) = self.review.analysis(&key, GRADE_VISITS)?
                && cached.suggestions.iter().any(|m| m.visits > 0)
            {
                // A deeper legacy root may lack moves; reuse metadata without
                // replacing that root's deeper score or redoing the bounded search.
                self.memo.get_mut(&key).unwrap().suggestions = cached.suggestions;
            } else if !self.refresh {
                self.memo.remove(&key);
                let query = self.query(doc, node, GRADE_VISITS)?;
                self.queue.push_back(Job {
                    query,
                    kind: Kind::Evaluation {
                        key,
                        position,
                        visits: GRADE_VISITS,
                    },
                });
            }
        }
        self.need(doc, doc.mainline[index + 1], GRADE_VISITS, self.preparing)
    }
    pub(super) fn prefetch_recorded(
        &mut self,
        doc: &Document,
        index: usize,
        plies: usize,
    ) -> Result<()> {
        let end = horizon(doc, index, plies).unwrap_or(*doc.mainline.last().unwrap());
        for &node in doc.mainline.iter().skip(index) {
            self.need(doc, node, GRADE_VISITS, false)?;
            if node == end {
                break;
            }
        }
        Ok(())
    }
    pub(super) fn need_attempt(&mut self, doc: &Document) -> Result<()> {
        self.need(doc, doc.selected, GRADE_VISITS, false)?;
        let (key, _) = self.keys(doc, doc.selected, GRADE_VISITS)?;
        // Newly played moves take priority over queued recorded-line prefetch.
        if let Some(index) = self
            .queue
            .iter()
            .position(|j| matches!(&j.kind, Kind::Evaluation {key:k,..} if *k == key))
        {
            let job = self.queue.remove(index).unwrap();
            self.queue.push_front(job);
        }
        Ok(())
    }
    pub(super) fn value(&self, doc: &Document, node: usize, visits: u64) -> Result<&Analysis> {
        self.memo
            .get(&self.keys(doc, node, visits)?.0)
            .ok_or_else(|| "Training evaluation is incomplete".into())
    }
}
