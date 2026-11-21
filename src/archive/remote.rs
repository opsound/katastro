//! Protocol-specific requests; storage and scheduling are shared.
use super::{Cancellation, Game, Server, fox, ogs, pandanet};
use crate::Result;
use reqwest::Url;
use std::time::Duration;
#[derive(Clone)]
pub(super) struct Entry {
    pub game: Game,
    pub url: Url,
}
pub(super) struct Page {
    pub entries: Vec<Entry>,
    pub total: usize,
}
#[derive(Clone)]
pub(super) enum Remote {
    Pandanet(pandanet::Remote),
    Fox(fox::Remote),
    Ogs(ogs::Remote),
}
impl Remote {
    pub fn new(endpoint: &str, server: Server) -> Result<Self> {
        Ok(match server {
            Server::Pandanet => Self::Pandanet(pandanet::Remote::new(endpoint)?),
            Server::Fox => Self::Fox(fox::Remote::new(endpoint)?),
            Server::Ogs => Self::Ogs(ogs::Remote::new(endpoint)?),
        })
    }
    pub fn set_interval(&mut self, interval: Duration) {
        if let Self::Pandanet(remote) = self {
            remote.interval = interval;
        }
    }
    pub fn reset(&mut self) {
        match self {
            Self::Pandanet(r) => r.reset(),
            Self::Fox(r) => r.reset(),
            Self::Ogs(r) => r.reset(),
        }
    }
    pub fn account(&mut self, username: &str, cancel: &Cancellation) -> Result<Option<String>> {
        match self {
            Self::Pandanet(_) => Ok(None),
            Self::Fox(r) => r.account(username, cancel).map(Some),
            Self::Ogs(r) => r.account(username, cancel).map(Some),
        }
    }
    pub fn page(
        &mut self,
        username: &str,
        password: &str,
        page: usize,
        cancel: &Cancellation,
    ) -> Result<Page> {
        match self {
            Self::Pandanet(r) => r.page(username, password, page, cancel),
            Self::Fox(r) => r.page(page, cancel),
            Self::Ogs(r) => r.page(page, cancel),
        }
    }
    pub fn download(&mut self, url: Url, cancel: &Cancellation) -> Result<Vec<u8>> {
        match self {
            Self::Pandanet(r) => r.download(url, cancel),
            Self::Fox(r) => r.download(url, cancel),
            Self::Ogs(r) => r.download(url, cancel),
        }
    }
}
