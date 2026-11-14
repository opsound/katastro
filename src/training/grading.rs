//! Own-move grading and progressively completed comparison reports.
use super::*;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct MoveLoss {
    pub score: f64,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Evaluation {
    pub score: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Comparison {
    /// Each curve is a contiguous evaluated prefix; missing tail values are omitted.
    pub recorded: Vec<Evaluation>,
    pub attempt: Vec<Evaluation>,
    #[serde(default)]
    pub total_plies: usize,
}
impl Comparison {
    pub fn complete(&self) -> bool {
        self.recorded.len() > self.total_plies && self.attempt.len() > self.total_plies
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub passed: bool,
    pub recorded_loss: MoveLoss,
    pub attempt_loss: MoveLoss,
    pub major_mistake: bool,
    #[serde(default)]
    pub comparison: Comparison,
}
impl Report {
    pub fn score_saved(&self) -> f64 {
        self.recorded_loss.score - self.attempt_loss.score
    }
}
impl Trainer {
    pub(super) fn grade(&mut self, original_end: usize) -> Result<()> {
        let p = self.problem.as_ref().unwrap();
        let doc = self.state.document.as_ref().unwrap();
        let (recorded_loss, _) =
            self.move_losses(&p.doc, p.doc.mainline[p.index], original_end, p.color)?;
        let (attempt_loss, major_mistake) =
            self.move_losses(doc, p.prefix.selected, doc.selected, p.color)?;
        let mut report = Report {
            recorded_loss,
            attempt_loss,
            major_mistake,
            passed: false,
            comparison: self.comparison(original_end)?,
        };
        report.passed =
            report.score_saved() >= 3.0 && (!self.settings.strict || !report.major_mistake);
        self.state.phase = if report.passed {
            Phase::Passed
        } else {
            Phase::Failed
        };
        self.state.status = if report.passed {
            "Passed · explore the suggested lines when ready"
        } else {
            "Not yet · retry without seeing the solution, or give up and explore"
        }
        .into();
        self.state.report = Some(report);
        self.report_id = Some(self.record(if self.state.phase == Phase::Passed {
            "passed"
        } else {
            "failed"
        })?);
        Ok(())
    }
    pub(super) fn original_end(&self) -> Result<usize> {
        let p = self.problem.as_ref().unwrap();
        let doc = self.state.document.as_ref().unwrap();
        let plies = doc.path(doc.selected)?.len() - p.prefix.nodes.len();
        horizon(&p.doc, p.index, plies)
            .ok_or_else(|| "Recorded game is too short for this attempt".into())
    }
    pub(super) fn ready_to_grade(&self, end: usize) -> Result<bool> {
        let p = self.problem.as_ref().unwrap();
        let doc = self.state.document.as_ref().unwrap();
        for (doc, start, end) in [
            (&p.doc, p.doc.mainline[p.index], end),
            (doc, p.prefix.selected, doc.selected),
        ] {
            for (before, after) in own_moves(doc, start, end, p.color)? {
                for node in [before, after] {
                    if !self
                        .memo
                        .contains_key(&self.keys(doc, node, GRADE_VISITS)?.0)
                    {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }
    fn comparison(&self, original_end: usize) -> Result<Comparison> {
        let p = self.problem.as_ref().unwrap();
        let doc = self.state.document.as_ref().unwrap();
        Ok(Comparison {
            recorded: self.comparison_line(
                &p.doc,
                p.doc.mainline[p.index],
                original_end,
                p.color,
            )?,
            attempt: self.comparison_line(doc, p.prefix.selected, doc.selected, p.color)?,
            total_plies: doc.path(doc.selected)?.len() - p.prefix.nodes.len(),
        })
    }
    pub(super) fn update_comparison(&mut self) -> Result<()> {
        let comparison = self.comparison(self.original_end()?)?;
        let report = self.state.report.as_mut().unwrap();
        if (
            report.comparison.recorded.len(),
            report.comparison.attempt.len(),
        ) != (comparison.recorded.len(), comparison.attempt.len())
        {
            report.comparison = comparison;
            self.review.reviews.execute(
                "UPDATE training_attempts SET report=?1 WHERE id=?2",
                params![
                    serde_json::to_string(report)?,
                    self.report_id.ok_or("Missing saved attempt")?
                ],
            )?;
        }
        Ok(())
    }
    fn move_losses(
        &self,
        doc: &Document,
        start: usize,
        end: usize,
        color: Color,
    ) -> Result<(MoveLoss, bool)> {
        let mut loss = MoveLoss::default();
        let mut major = false;
        for (before, after) in own_moves(doc, start, end, color)? {
            let a = score(self.value(doc, before, GRADE_VISITS)?, color);
            let b = score(self.value(doc, after, GRADE_VISITS)?, color);
            let score = (a - b).max(0.0);
            loss.score += score;
            major |= score >= MAJOR_LOSS;
        }
        Ok((loss, major))
    }
    fn comparison_line(
        &self,
        doc: &Document,
        start: usize,
        end: usize,
        color: Color,
    ) -> Result<Vec<Evaluation>> {
        let path = doc.path(end)?;
        let first = path
            .iter()
            .position(|id| *id == start)
            .ok_or("Missing exercise position")?;
        let mut values = Vec::new();
        for (_, node) in path[first..]
            .iter()
            .enumerate()
            .filter(|(i, node)| *i == 0 || doc.nodes[**node].played.is_some())
        {
            let Some(value) = self.memo.get(&self.keys(doc, *node, GRADE_VISITS)?.0) else {
                break;
            };
            values.push(Evaluation {
                score: score(value, color),
            });
        }
        Ok(values)
    }
    pub(super) fn record(&self, outcome: &str) -> Result<i64> {
        self.review.reviews.execute(
            "INSERT INTO training_attempts (problem,group_key,outcome,report,document) VALUES (?1,?2,?3,?4,?5)",
            params![
                self.problem.as_ref().unwrap().key,
                self.problem.as_ref().unwrap().group,
                outcome,
                serde_json::to_string(&self.state.report)?,
                serde_json::to_string(&self.state.document)?
            ],
        )?;
        Ok(self.review.reviews.last_insert_rowid())
    }
}

fn own_moves(
    doc: &Document,
    start: usize,
    end: usize,
    color: Color,
) -> Result<Vec<(usize, usize)>> {
    let path = doc.path(end)?;
    let first = path
        .iter()
        .position(|id| *id == start)
        .ok_or("The attempt is not descended from its starting position")?;
    Ok(path[first..]
        .windows(2)
        .filter(|pair| doc.nodes[pair[1]].played.is_some_and(|m| m.color == color))
        .map(|pair| (pair[0], pair[1]))
        .collect())
}
