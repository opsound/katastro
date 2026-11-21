//! Unauthenticated player lookup, completed game history and original SGFs from OGS.
use super::{
    Cancellation, Game, Server,
    http::Http,
    remote::{Entry, Page},
};
use crate::Result;
use reqwest::Url;
use serde::{Deserialize, de::DeserializeOwned};
use std::{collections::HashSet, sync::Arc};

pub(super) const ENDPOINT: &str = "https://online-go.com/";
const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 10_000;

#[derive(Clone)]
pub(super) struct Remote {
    endpoint: Url,
    client: Http,
    player: u64,
    seen: Arc<HashSet<u64>>,
}
#[derive(Deserialize)]
struct Listing<T> {
    count: usize,
    results: Vec<T>,
    next: Option<String>,
}
#[derive(Deserialize)]
struct Player {
    id: u64,
    username: String,
}
#[derive(Deserialize, Default)]
struct Players {
    black: Option<Player>,
    white: Option<Player>,
}
#[derive(Deserialize)]
struct Record {
    id: u64,
    black: Option<u64>,
    white: Option<u64>,
    #[serde(default)]
    players: Players,
    rengo_black_team: Option<Vec<Option<u64>>>,
    rengo_white_team: Option<Vec<Option<u64>>>,
    ended: Option<String>,
    mode: Option<String>,
    #[serde(default)]
    private: bool,
}
impl Record {
    fn includes(&self, player: u64) -> bool {
        [
            self.black
                .or_else(|| self.players.black.as_ref().map(|p| p.id)),
            self.white
                .or_else(|| self.players.white.as_ref().map(|p| p.id)),
        ]
        .contains(&Some(player))
            || [&self.rengo_black_team, &self.rengo_white_team]
                .into_iter()
                .flatten()
                .any(|team| team.contains(&Some(player)))
    }
}
impl Remote {
    pub fn new(endpoint: &str) -> Result<Self> {
        let endpoint = Url::parse(endpoint)?;
        let local = endpoint.scheme() == "http"
            && endpoint.host_str() == Some("127.0.0.1")
            && endpoint.username().is_empty()
            && endpoint.password().is_none();
        if !local && endpoint.as_str() != ENDPOINT {
            return Err("Unsupported OGS archive endpoint".into());
        }
        Ok(Self {
            endpoint,
            client: Http::new(Server::Ogs)?,
            player: 0,
            seen: Arc::default(),
        })
    }
    pub fn reset(&mut self) {
        Arc::make_mut(&mut self.seen).clear();
    }
    pub fn download(&self, url: Url, cancel: &Cancellation) -> Result<Vec<u8>> {
        cancel.check()?;
        let response = self.client.send(url, None)?;
        cancel.check()?;
        let bytes = self.client.read(response)?;
        cancel.check()?;
        Ok(bytes)
    }
    fn listing<T: DeserializeOwned>(
        &self,
        mut url: Url,
        page: usize,
        cancel: &Cancellation,
    ) -> Result<(Vec<T>, usize)> {
        url.query_pairs_mut()
            .append_pair("page_size", &PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string());
        let bytes = self.download(url.clone(), cancel)?;
        let data: Listing<T> = serde_json::from_slice(&bytes)
            .map_err(|_| "Could not read the OGS archive response; the service may have changed")?;
        let pages = if let Some(next) = data.next {
            // Check the continuation, then rebuild the request with our original filters.
            let next = url
                .join(&next)
                .map_err(|_| "Unexpected OGS archive pagination")?;
            if next.origin() != url.origin()
                || !next.username().is_empty()
                || next.password().is_some()
                || next.path().trim_end_matches('/') != url.path().trim_end_matches('/')
                || next
                    .query_pairs()
                    .find(|(k, _)| k == "page")
                    .and_then(|(_, v)| v.parse().ok())
                    != Some(page + 1)
                || page >= MAX_PAGES
                || data.results.is_empty()
            {
                return Err("Unexpected OGS archive pagination".into());
            }
            data.count.div_ceil(PAGE_SIZE).clamp(page + 1, MAX_PAGES)
        } else {
            page
        };
        Ok((data.results, pages))
    }
    pub fn account(&mut self, username: &str, cancel: &Cancellation) -> Result<String> {
        let mut url = self.endpoint.join("/api/v1/players/")?;
        url.query_pairs_mut()
            .append_pair("username__istartswith", username)
            .append_pair("ordering", "username");
        let folded = username.to_lowercase();
        let mut page = 1;
        loop {
            let (players, pages) = self.listing::<Player>(url.clone(), page, cancel)?;
            if let Some(exact) = players
                .into_iter()
                .find(|p| p.username.to_lowercase() == folded && p.id > 0)
            {
                self.player = exact.id;
                return Ok(exact.username);
            }
            if page == pages {
                return Err("OGS username was not found".into());
            }
            page += 1;
        }
    }
    pub fn page(&mut self, page: usize, cancel: &Cancellation) -> Result<Page> {
        let mut url = self
            .endpoint
            .join(&format!("/api/v1/players/{}/games/", self.player))?;
        url.query_pairs_mut().extend_pairs([
            ("mode", "game"),
            ("ended__isnull", "false"),
            ("ordering", "-ended"),
        ]);
        let (records, total) = self.listing::<Record>(url, page, cancel)?;
        let mut entries = Vec::new();
        let seen = Arc::make_mut(&mut self.seen);
        let previous = seen.len();
        let nonempty = !records.is_empty();
        for record in records {
            if !seen.insert(record.id) {
                continue;
            }
            let Some(ended) = &record.ended else {
                continue;
            };
            if ended.is_empty()
                || record.private
                || record.mode.as_deref().is_some_and(|mode| mode != "game")
            {
                continue;
            }
            if !record.includes(self.player) {
                return Err("OGS returned another player's game; sync stopped".into());
            }
            let date = ended
                .get(..19)
                .ok_or("Invalid OGS game date")?
                .replace('-', "/")
                .replace('T', " ");
            entries.push(Entry {
                url: self
                    .endpoint
                    .join(&format!("/api/v1/games/{}/sgf", record.id))?,
                game: Game {
                    id: record.id.to_string(),
                    date,
                    black: record.players.black.map(|p| p.username).unwrap_or_default(),
                    white: record.players.white.map(|p| p.username).unwrap_or_default(),
                    result: String::new(),
                    path: Default::default(),
                },
            });
        }
        if nonempty && previous == seen.len() {
            return Err("OGS returned repeated archive pagination".into());
        }
        Ok(Page { entries, total })
    }
}
