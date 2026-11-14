//! Suggested lines and persistent private review branches.
use super::*;
impl Trainer {
    pub fn reveal(&mut self) -> Result<()> {
        if !matches!(
            self.state.phase,
            Phase::YourTurn | Phase::Opponent | Phase::Grading | Phase::Failed | Phase::Passed
        ) {
            return Err("There is no exercise to reveal".into());
        }
        if self.state.phase != Phase::Passed {
            self.record("given-up")?;
        }
        self.queue.clear();
        self.pending.clear();
        self.state.phase = Phase::Revealing;
        self.state.status = "Preparing suggested lines…".into();
        let p = self.problem.as_ref().unwrap();
        let doc = p.doc.clone();
        let node = doc.mainline[p.index];
        let mut query = self.query(&doc, node, GRADE_VISITS)?;
        query["overrideSettings"] = json!({"analysisPVLen":12});
        self.queue.push_back(Job {
            query,
            kind: Kind::Lines,
        });
        Ok(())
    }
    pub(super) fn finish_reveal(&mut self, reply: &Value) -> Result<()> {
        let p = self.problem.as_ref().unwrap();
        let path = self.output.join(format!("{}.sgf", p.key));
        let mut doc = p.doc.clone();
        if path.is_file() {
            self.review.import(&path)?;
            let saved = self.review.document().unwrap();
            if saved.position(*saved.mainline.last().unwrap())?
                != doc.position(*doc.mainline.last().unwrap())?
            {
                return Err(
                    "The training review's original game has changed; it was not overwritten"
                        .into(),
                );
            }
            doc = saved.clone();
        }
        let anchor = doc.mainline[p.index];
        let records = self
            .review
            .reviews
            .prepare("SELECT document FROM training_attempts WHERE problem=?1 ORDER BY id")?
            .query_map([&p.key], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for record in records {
            let attempt: Document = serde_json::from_str(&record)?;
            doc.selected = anchor;
            for &id in attempt
                .path(attempt.selected)?
                .iter()
                .skip(p.prefix.nodes.len())
            {
                doc.append_move(attempt.nodes[id].played.unwrap().point)?;
            }
        }
        let infos = reply["moveInfos"]
            .as_array()
            .ok_or("KataGo returned no suggested lines")?;
        let mut candidates = infos
            .iter()
            .filter_map(|m| Some((m, m["visits"].as_u64()?, m["scoreLead"].as_f64()?)))
            .filter(|(_, visits, score)| *visits >= 16 && score.is_finite())
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, visits, _)| std::cmp::Reverse(*visits));
        let Some((_, _, score)) = candidates.first().copied() else {
            return Err("KataGo has not searched a suggested line deeply enough".into());
        };
        let mut lines = Vec::new();
        for (m, _, s) in candidates {
            if p.color.perspective(score - s) > 2.0 {
                continue;
            }
            let Some(pv) = m["pv"].as_array().filter(|pv| pv.len() >= 2) else {
                continue;
            };
            let points = pv
                .iter()
                .take(12)
                .map(|value| Point::from_gtp(value.as_str().ok_or("Invalid PV move")?, doc.size))
                .collect::<Result<Vec<_>>>();
            let Ok(points) = points else {
                continue;
            };
            let mut line = doc.clone();
            line.selected = anchor;
            if points
                .iter()
                .try_for_each(|point| line.append_move(*point).map(|_| ()))
                .is_ok()
            {
                let text = points
                    .iter()
                    .map(|p| p.map(|p| p.gtp(line.size)).unwrap_or("pass".into()))
                    .collect::<Vec<_>>()
                    .join(" ");
                let comment = line.nodes[anchor].property("C").unwrap_or("").to_owned();
                line.nodes[anchor].props.insert(
                    "C".into(),
                    vec![format!(
                        "{comment}\nSuggested line {}: {text}",
                        lines.len() + 1
                    )],
                );
                doc = line;
                lines.push(points);
                if lines.len() == 3 {
                    break;
                }
            }
        }
        if lines.is_empty() {
            return Err(
                "KataGo returned no legal multi-move suggestions; try revealing again".into(),
            );
        }
        std::fs::create_dir_all(&self.output)?;
        let temp = path.with_extension("sgf.tmp");
        std::fs::write(&temp, doc.to_sgf())?;
        std::fs::rename(temp, &path)?;
        let count = lines.len();
        self.state.lines = lines;
        self.state.reveal = Some((path, p.index));
        self.state.phase = Phase::Revealed;
        self.state.status = format!("{count} suggested lines · original game remains on top");
        Ok(())
    }
}
