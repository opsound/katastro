//! Derived discovery results live with the evictable analysis, not user progress.
use super::*;
use rusqlite::OptionalExtension;

pub(super) struct Game {
    doc: Document,
    candidates: VecDeque<usize>,
    screened: bool,
    key: String,
    entry: Entry,
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Entry {
    pub candidates: Vec<usize>,
    pub complete: bool,
}
impl Entry {
    pub fn key(doc: &Document, account: &str, profile: &EngineProfile) -> Result<String> {
        Ok(digest(&serde_json::to_vec(&(
            POLICY,
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
    fn eligible(&self, doc: &Document, index: usize) -> Result<bool> {
        let key = self.problem_key(doc, index)?;
        Ok(horizon(doc, index, self.settings.moves * 2).is_some()
            && !self.seen.contains(&key)
            && (self.settings.include_completed || !self.completed(&key)?))
    }
    pub(super) fn find_prepared(&mut self) -> Result<()> {
        // Look across the selected library before starting any new engine work.
        for path in self.library.clone() {
            let Some(doc) = self.read_game(&path) else {
                continue;
            };
            let key = catalog::Entry::key(&doc, &self.account, &self.profile)?;
            for index in catalog::Entry::load(&self.review.cache, &key)?.candidates {
                if self.eligible(&doc, index)? {
                    return self.start_problem(doc, index);
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
            });
            return Ok(());
        }
        let doc = self.game.as_ref().unwrap().doc.clone();
        let color = player_color(&doc, &self.account).unwrap();
        if let Some(index) = self.confirming.take()
            && self.qualifies(&doc, index, color, GRADE_VISITS, MAJOR_LOSS)?
        {
            let game = self.game.as_mut().unwrap();
            if !game.entry.candidates.contains(&index) {
                game.entry.candidates.push(index);
            }
            game.entry.save(&self.review.cache, &game.key)?;
            if !self.preparing && self.eligible(&doc, index)? {
                self.start_problem(doc, index)?;
                return Ok(());
            }
        }
        if !self.game.as_ref().unwrap().screened {
            // Coarse estimates get a wider net; confirmation requires five points.
            let mut candidates = Vec::new();
            for index in 0..doc.mainline.len().saturating_sub(1) {
                if horizon(&doc, index, 2).is_some()
                    && self.qualifies(&doc, index, color, SCREEN_VISITS, 3.0)?
                {
                    let a = self.value(&doc, doc.mainline[index], SCREEN_VISITS)?;
                    let b = self.value(&doc, doc.mainline[index + 1], SCREEN_VISITS)?;
                    candidates.push((index, score(a, color) - score(b, color)));
                }
            }
            candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
            let game = self.game.as_mut().unwrap();
            game.candidates = candidates.into_iter().map(|(i, _)| i).collect();
            game.screened = true;
        }
        if let Some(index) = self.game.as_mut().unwrap().candidates.pop_front() {
            self.confirming = Some(index);
            for node in [doc.mainline[index], doc.mainline[index + 1]] {
                self.need(&doc, node, GRADE_VISITS, self.preparing)?;
            }
        } else {
            let mut game = self.game.take().unwrap();
            game.entry.complete = true;
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
            for &index in &entry.candidates {
                self.prefetch_recorded(doc, index, Settings::default().moves * 2)?;
            }
        }
        Ok(())
    }
}

impl Trainer {
    fn qualifies(
        &self,
        doc: &Document,
        index: usize,
        color: Color,
        visits: u64,
        loss: f64,
    ) -> Result<bool> {
        if doc.nodes[doc.mainline[index + 1]]
            .played
            .is_none_or(|m| m.color != color)
            || doc.board(doc.mainline[index])?.next != color
        {
            return Ok(false);
        }
        let a = score(self.value(doc, doc.mainline[index], visits)?, color);
        let b = score(self.value(doc, doc.mainline[index + 1], visits)?, color);
        Ok(a - b >= loss)
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
    fn completed(&self, key: &str) -> Result<bool> {
        Ok(self.db.query_row("SELECT EXISTS(SELECT 1 FROM training_attempts WHERE problem=?1 AND outcome IN ('passed','given-up'))",[key],|r|r.get(0))?)
    }
}
