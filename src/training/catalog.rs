//! Derived discovery results live with the evictable analysis, not user progress.
use super::*;
use rand::seq::SliceRandom;
use rusqlite::OptionalExtension;

pub(super) struct Game {
    doc: Document,
    candidates: VecDeque<usize>,
    screened: bool,
    key: String,
    entry: Entry,
    deferred: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Candidate {
    pub index: usize,
    pub group: String,
}
#[derive(Default, Serialize, Deserialize)]
pub(super) struct Entry {
    pub candidates: Vec<Candidate>,
    pub complete: bool,
    pub pending_completions: bool,
}
impl Entry {
    pub fn key(doc: &Document, account: &str, profile: &EngineProfile) -> Result<String> {
        Ok(digest(&serde_json::to_vec(&(
            "training-candidates-score-v3",
            doc.source_identity()?,
            &doc.mainline,
            account.to_lowercase(),
            player_color(doc, account),
            profile,
        ))?))
    }
    pub fn load(db: &Connection, key: &str) -> Result<Self> {
        let value: Option<String> = db
            .query_row(
                "SELECT result FROM training_games WHERE key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or_default())
    }
    pub fn save(&self, db: &Connection, key: &str) -> Result<()> {
        db.execute("INSERT INTO training_games(key,result) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET result=excluded.result", params![key,serde_json::to_string(self)?])?;
        Ok(())
    }
}

impl Trainer {
    /// Prepare every candidate without starting an attempt or loading HumanSL.
    /// Refresh uses the latest compatible review values, including deeper searches.
    pub fn prepare(&mut self, games: Vec<PathBuf>, account: String, refresh: bool) -> Result<()> {
        self.initialize(
            games,
            account,
            Settings {
                moves: 1,
                ..Settings::default()
            },
        )?;
        self.preparing = true;
        self.refresh = refresh;
        self.advance()
    }
    fn read_game(&self, path: &Path) -> Option<Document> {
        let doc = Document::parse(&std::fs::read(path).ok()?).ok()?;
        (player_color(&doc, &self.account).is_some() && doc.position(*doc.mainline.last()?).is_ok())
            .then_some(doc)
    }
    fn eligible(&self, doc: &Document, candidate: &Candidate) -> Result<bool> {
        let key = self.problem_key(doc, candidate.index)?;
        Ok(
            horizon(doc, candidate.index, self.settings.moves * 2).is_some()
                && !self.seen.contains(&candidate.group)
                && (self.settings.include_completed || !self.completed(&key, &candidate.group)?),
        )
    }
    pub(super) fn find_prepared(&mut self) -> Result<()> {
        // Look across the selected library before starting any new engine work.
        let mut games = self.library.clone();
        games.shuffle(&mut self.rng);
        self.games = games.clone().into();
        for path in games {
            let Some(doc) = self.read_game(&path) else {
                continue;
            };
            let key = catalog::Entry::key(&doc, &self.account, &self.profile)?;
            let entry = catalog::Entry::load(&self.review.cache, &key)?;
            if entry.pending_completions {
                continue;
            }
            for candidate in entry.candidates {
                if self.eligible(&doc, &candidate)? {
                    return self.start_problem(doc, candidate);
                }
            }
        }
        self.advance()
    }
    pub(super) fn search_next(&mut self) -> Result<()> {
        if self.game.is_none() {
            let Some(path) = self.games.pop_front() else {
                self.state.phase = Phase::Idle;
                self.state.status = if self.preparing {
                    format!(
                        "Training discovery complete · {} games checked",
                        self.scanned
                    )
                } else {
                    format!(
                        "No more qualifying positions in {} games ({} unreadable, unsupported, or for another player). Try more recent games or a shorter attempt.",
                        self.scanned, self.skipped
                    )
                };
                return Ok(());
            };
            self.scanned += 1;
            self.state.status = format!(
                "Finding point losses · game {} · {GRADE_VISITS}-visit confirmation",
                self.scanned
            );
            let Some(doc) = self.read_game(&path) else {
                self.skipped += 1;
                return Ok(());
            };
            let key = catalog::Entry::key(&doc, &self.account, &self.profile)?;
            let entry = if self.refresh {
                catalog::Entry::default()
            } else {
                catalog::Entry::load(&self.review.cache, &key)?
            };
            if entry.complete {
                self.warm_recorded(&doc, &entry)?;
                return Ok(());
            }
            entry.save(&self.review.cache, &key)?;
            for &node in &doc.mainline {
                self.need(&doc, node, SCREEN_VISITS, self.preparing)?;
            }
            self.game = Some(Game {
                doc,
                candidates: VecDeque::new(),
                screened: false,
                key,
                entry,
                deferred: false,
            });
            return Ok(());
        }
        let doc = self.game.as_ref().unwrap().doc.clone();
        let color = player_color(&doc, &self.account).unwrap();
        if let Some(index) = self.confirming.take()
            && self
                .qualifying_loss(&doc, index, color, GRADE_VISITS, MAJOR_LOSS)?
                .is_some()
        {
            if let Some(candidate) = self.candidate(&doc, index)? {
                // Backfill grouping for legacy attempts without changing their grades.
                self.review.reviews.execute(
                    "UPDATE training_attempts SET group_key=?1 WHERE problem=?2 AND group_key IS NULL",
                    params![candidate.group, self.problem_key(&doc, index)?],
                )?;
                let game = self.game.as_mut().unwrap();
                if !game.entry.candidates.iter().any(|c| c.index == index) {
                    game.entry.candidates.push(candidate.clone());
                }
                game.entry.save(&self.review.cache, &game.key)?;
                if !self.preparing && self.eligible(&doc, &candidate)? {
                    self.start_problem(doc, candidate)?;
                    return Ok(());
                }
            } else if self.refresh {
                // Completed-review indexing never starts engine work. Idle/foreground
                // discovery will backfill older root-only analysis when needed.
                let completed = self.completed(&self.problem_key(&doc, index)?, "")?;
                let game = self.game.as_mut().unwrap();
                game.deferred = true;
                game.entry.pending_completions |= completed;
            } else {
                return Err("KataGo returned no searched move for this training position".into());
            }
        }
        if !self.game.as_ref().unwrap().screened {
            // Coarse estimates get a wider net; confirmation requires five points.
            let legacy: HashSet<String> = self.review.reviews.prepare(
                "SELECT problem FROM training_attempts WHERE group_key IS NULL AND outcome IN ('passed','given-up')",
            )?.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
            let mut candidates = Vec::new();
            for index in 0..doc.mainline.len().saturating_sub(1) {
                if horizon(&doc, index, 2).is_some()
                    && let Some(loss) =
                        self.qualifying_loss(&doc, index, color, SCREEN_VISITS, 3.0)?
                {
                    let completed =
                        !legacy.is_empty() && legacy.contains(&self.problem_key(&doc, index)?);
                    candidates.push((index, loss, completed));
                }
            }
            // Map old completions first, even if another occurrence has a larger loss.
            candidates.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.total_cmp(&a.1)));
            let game = self.game.as_mut().unwrap();
            game.candidates = candidates.into_iter().map(|(i, _, _)| i).collect();
            game.screened = true;
        }
        if let Some(index) = self.game.as_mut().unwrap().candidates.pop_front() {
            self.confirming = Some(index);
            self.need_candidate(&doc, index)?;
        } else {
            let mut game = self.game.take().unwrap();
            game.entry.complete = !game.deferred;
            game.entry.pending_completions &= game.deferred;
            game.entry.save(&self.review.cache, &game.key)?;
            self.warm_recorded(&doc, &game.entry)?;
        }

        Ok(())
    }
    fn warm_recorded(&mut self, doc: &Document, entry: &Entry) -> Result<()> {
        if self.preparing && !self.refresh {
            // Discovery can use deeper review values. Grading retains its own
            // requested-budget cache; do not carry discovery aliases into it.
            self.memo.clear();
            let plies = Settings::default().moves * 2;
            let mut warmed = HashSet::new();
            // Prefer a full default attempt; retain a shorter occurrence when needed.
            for candidate in entry
                .candidates
                .iter()
                .filter(|c| horizon(doc, c.index, plies).is_some())
                .chain(&entry.candidates)
            {
                if warmed.insert(&candidate.group) {
                    self.prefetch_recorded(doc, candidate.index, plies)?;
                }
            }
        }
        Ok(())
    }
    fn qualifying_loss(
        &self,
        doc: &Document,
        index: usize,
        color: Color,
        visits: u64,
        threshold: f64,
    ) -> Result<Option<f64>> {
        if doc.nodes[doc.mainline[index + 1]]
            .played
            .is_none_or(|m| m.color != color)
            || doc.board(doc.mainline[index])?.next != color
        {
            return Ok(None);
        }
        let a = score(self.value(doc, doc.mainline[index], visits)?, color);
        let b = score(self.value(doc, doc.mainline[index + 1], visits)?, color);
        let loss = a - b;
        Ok((loss >= threshold).then_some(loss))
    }
    pub(super) fn problem_key(&self, doc: &Document, index: usize) -> Result<String> {
        Ok(digest(&serde_json::to_vec(&(
            POLICY,
            self.account.to_lowercase(),
            doc.source_identity()?,
            doc.position(doc.mainline[index])?,
            &self.profile,
        ))?))
    }
    fn completed(&self, key: &str, group: &str) -> Result<bool> {
        Ok(self.review.reviews.query_row("SELECT EXISTS(SELECT 1 FROM training_attempts WHERE (problem=?1 OR group_key=?2) AND outcome IN ('passed','given-up'))",[key, group],|r|r.get(0))?)
    }
    fn candidate(&self, doc: &Document, index: usize) -> Result<Option<Candidate>> {
        let value = self.value(doc, doc.mainline[index], GRADE_VISITS)?;
        let Some(best) = value.suggestions.iter().find(|m| m.visits > 0) else {
            return Ok(None);
        };
        // A played/captured intersection can become a new opportunity later.
        let last_played = best.point.and_then(|point| {
            doc.mainline[..=index].iter().rposition(|id| {
                doc.nodes[*id]
                    .played
                    .is_some_and(|m| m.point == Some(point))
            })
        });
        let group = digest(&serde_json::to_vec(&(
            "training-opportunity-v1",
            POLICY,
            doc.source_identity()?,
            self.account.to_lowercase(),
            player_color(doc, &self.account),
            &self.profile,
            best.point,
            last_played,
        ))?);
        Ok(Some(Candidate { index, group }))
    }
}
