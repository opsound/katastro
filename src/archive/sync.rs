//! Bounded archive scanning, concurrent downloads, and retry policy.
use super::*;
impl Service {
    pub(super) fn sync(
        &mut self,
        now: u64,
        cancel: &Cancellation,
        progress: &mut dyn FnMut(Snapshot),
    ) -> Result<()> {
        self.next_due = now.saturating_add(SYNC_INTERVAL);
        self.state.error = None;
        if self.state.server == Server::Pandanet && self.password.is_none() && self.state.remember {
            self.password = self
                .credentials
                .load(&self.state.username)?
                .map(Zeroizing::new);
        }
        if self.state.server == Server::Pandanet && self.password.is_none() {
            self.needs_login = true;
            return Err("Pandanet sign-in required to sync games".into());
        }
        self.state.syncing = true;
        self.state.downloaded = 0;
        self.state.page = 0;
        self.state.pages = 0;
        self.state.status = format!("Connecting to {}", self.state.server.label());
        progress(self.snapshot());
        self.remote.reset();
        if let Some(account) = self.remote.account(&self.state.username, cancel)? {
            self.state.username = account;
            self.load_account()?;
        }
        let full = !self.state.failures.is_empty()
            || self
                .last_full
                .is_none_or(|last| now.saturating_sub(last) >= 24 * 60 * 60);
        let mut page_number = 1;
        let mut scanned = 0usize;
        let mut failed = 0usize;
        let mut seen = std::collections::HashSet::new();
        loop {
            cancel.check()?;
            let page = self.remote.page(
                &self.state.username,
                self.password.as_deref().map(|p| p.as_str()).unwrap_or(""),
                page_number,
                cancel,
            )?;
            cancel.check()?;
            self.needs_login = false;
            if self.remember_pending {
                if self.state.server == Server::Pandanet {
                    if self.state.remember {
                        self.credentials
                            .save(&self.state.username, self.password.as_deref().unwrap())?;
                    } else {
                        self.credentials.forget(&self.state.username)?;
                    }
                }
                self.save_config()?;
                self.remember_pending = false;
            }
            self.state.page = page_number;
            self.state.pages = page.total;
            self.state.status = format!("Syncing page {page_number} of {}", page.total);
            progress(self.snapshot());
            let remaining = self
                .state
                .limit
                .map_or(usize::MAX, |limit| limit.saturating_sub(scanned));
            let entries: Vec<_> = page.entries.into_iter().take(remaining).collect();
            let has_entries = !entries.is_empty();
            scanned += entries.len();
            let pending: Vec<_> = entries
                .into_iter()
                .filter(|entry| {
                    !self
                        .state
                        .games
                        .iter()
                        .any(|game| game.id == entry.game.id && game.path.is_file())
                })
                .filter(|entry| seen.insert(entry.game.id.clone()))
                .collect();
            let all_known = has_entries && pending.is_empty();
            for batch in pending.chunks(self.parallel_downloads) {
                cancel.check()?;
                let results = if self.parallel_downloads == 1 {
                    batch
                        .iter()
                        .map(|entry| self.remote.download(entry.url.clone(), cancel))
                        .collect::<Vec<_>>()
                } else {
                    std::thread::scope(|scope| {
                        let handles: Vec<_> = batch
                            .iter()
                            .map(|entry| {
                                let mut remote = self.remote.clone();
                                scope.spawn(move || remote.download(entry.url.clone(), cancel))
                            })
                            .collect();
                        handles
                            .into_iter()
                            .map(|h| {
                                h.join().unwrap_or_else(|_| {
                                    Err("Archive download worker stopped".into())
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                };
                let mut backoff = None;
                for (entry, result) in batch.iter().zip(results) {
                    cancel.check()?;
                    let result =
                        result.and_then(|bytes| self.save_download(entry.game.clone(), &bytes));
                    match result {
                        Ok(()) => self.state.downloaded += 1,
                        Err(error) => {
                            let message = error.to_string();
                            if message.contains("HTTP 429") || message.contains("HTTP 503") {
                                backoff = Some(message.clone());
                            }
                            self.db.execute("INSERT INTO failures VALUES (?1,?2,?3,?4) ON CONFLICT(username,id) DO UPDATE SET message=excluded.message",params![self.state.username,entry.game.id,entry.game.date,message])?;
                            self.load_account()?;
                            failed += 1;
                        }
                    }
                    progress(self.snapshot());
                }
                if let Some(message) = backoff {
                    return Err(message.into());
                }
            }
            cancel.check()?;
            if page_number >= page.total
                || self.state.limit.is_some_and(|limit| scanned >= limit)
                || (!full && all_known)
            {
                break;
            }
            page_number += 1;
        }
        if failed > 0 {
            return Err(format!("{failed} games could not be downloaded; other games are saved. Failed games will be retried on the next sync.").into());
        }
        if full {
            self.last_full = Some(now);
        }
        self.db.execute("INSERT INTO accounts VALUES (?1,?2,?3) ON CONFLICT(username) DO UPDATE SET last_sync=excluded.last_sync,last_full=excluded.last_full",params![self.state.username,now as i64,self.last_full.map(|t|t as i64)])?;
        self.state.last_sync = Some(now);
        self.state.status = format!(
            "Up to date · {} games · {} downloaded",
            self.state.games.len(),
            self.state.downloaded
        );
        Ok(())
    }
}
