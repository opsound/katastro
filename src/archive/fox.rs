//! Fox's public web archive. No login, third-party proxy, or client credentials.
use super::{
    Cancellation, Game,
    remote::{Entry, Page},
};
use crate::Result;
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde_json::Value;
use std::{collections::HashSet, io::Read, time::Duration};
pub(super) const ENDPOINT: &str = "https://newframe.foxwq.com/";
#[derive(Clone)]
pub(super) struct Remote {
    profile: Url,
    records: Url,
    client: Client,
    uid: String,
    cursor: String,
    seen: HashSet<String>,
}
fn field(value: &Value, key: &str) -> String {
    match &value[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}
impl Remote {
    pub fn new(endpoint: &str) -> Result<Self> {
        let profile = Url::parse(endpoint)?;
        let local = profile.scheme() == "http" && profile.host_str() == Some("127.0.0.1");
        if !local && profile.as_str() != ENDPOINT {
            return Err("Unsupported Fox archive endpoint".into());
        }
        let records = if local {
            profile.clone()
        } else {
            Url::parse("https://h5.foxwq.com/")?
        };
        Ok(Self {
            profile,
            records,
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .connect_timeout(Duration::from_secs(10))
                .redirect(Policy::none())
                .referer(false)
                .user_agent("Katastro/0.1 (personal game archive sync)")
                .build()?,
            uid: String::new(),
            cursor: "0".into(),
            seen: HashSet::new(),
        })
    }
    pub fn reset(&mut self) {
        self.cursor = "0".into();
        self.seen.clear();
    }
    fn request(&self, url: Url, cancel: &Cancellation) -> Result<Value> {
        cancel.check()?;
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|_| "Could not connect to Fox; sync will retry")?;
        cancel.check()?;
        if !response.status().is_success() {
            return Err(format!(
                "Fox returned HTTP {}; sync will retry",
                response.status().as_u16()
            )
            .into());
        }
        let mut bytes = Vec::new();
        const LIMIT: u64 = 16 * 1024 * 1024;
        response
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read Fox response")?;
        cancel.check()?;
        if bytes.len() as u64 > LIMIT {
            return Err("Fox response exceeds the download limit".into());
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "Could not read Fox's archive response; the service may have changed")?;
        if value["result"].as_i64() != Some(0) {
            return Err(
                "Fox could not return these public records; check the username or try again later"
                    .into(),
            );
        }
        Ok(value)
    }
    pub fn account(&mut self, username: &str, cancel: &Cancellation) -> Result<String> {
        let mut url = self.profile.join("/cgi/QueryUserInfoPanel")?;
        url.query_pairs_mut()
            .extend_pairs([("srcuid", "0"), ("username", username)]);
        let value = self.request(url, cancel)?;
        self.uid = field(&value, "uid");
        let account = field(&value, "username");
        if self.uid.is_empty()
            || !self.uid.bytes().all(|b| b.is_ascii_digit())
            || account.is_empty()
        {
            return Err("Fox username was not found".into());
        }
        if value["hide_game_record"].as_i64().is_some_and(|v| v != 0) {
            return Err("This Fox account's game records are private".into());
        }
        Ok(account)
    }
    pub fn page(&mut self, page: usize, cancel: &Cancellation) -> Result<Page> {
        let mut url = self
            .records
            .join("/yehuDiamond/chessbook_local/YHWQFetchChessList")?;
        url.query_pairs_mut().extend_pairs([
            ("dstuid", self.uid.as_str()),
            ("type", "1"),
            ("lastCode", self.cursor.as_str()),
        ]);
        let value = self.request(url, cancel)?;
        if field(&value, "dstuid") != self.uid {
            return Err("Fox returned another player's archive; sync stopped".into());
        }
        let games = value["chesslist"]
            .as_array()
            .ok_or("Fox returned no game list")?;
        let mut entries = Vec::new();
        for game in games {
            let id = field(game, "chessid");
            if id.is_empty()
                || !id.bytes().all(|b| b.is_ascii_digit())
                || !self.seen.insert(id.clone())
            {
                return Err("Fox returned unexpected archive pagination".into());
            }
            if field(game, "blackuid") != self.uid && field(game, "whiteuid") != self.uid {
                return Err("Fox returned another player's game; sync stopped".into());
            }
            let mut url = self
                .records
                .join("/yehuDiamond/chessbook_local/YHWQFetchChess")?;
            url.query_pairs_mut().append_pair("chessid", &id);
            entries.push(Entry {
                url,
                game: Game {
                    id: id.clone(),
                    date: field(game, "starttime").replace('-', "/"),
                    white: field(game, "whitenick"),
                    black: field(game, "blacknick"),
                    result: String::new(),
                    path: Default::default(),
                },
            });
            self.cursor = id;
        }
        // The endpoint has no total. A full page can have more than 100 records.
        Ok(Page {
            total: if games.len() >= 100 { page + 1 } else { page },
            entries,
        })
    }
    pub fn download(&self, url: Url, cancel: &Cancellation) -> Result<Vec<u8>> {
        let id = url
            .query_pairs()
            .find(|(k, _)| k == "chessid")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let value = self.request(url, cancel)?;
        if field(&value, "chessid") != id {
            return Err("Fox returned a different game; it was not saved".into());
        }
        Ok(value["chess"]
            .as_str()
            .ok_or("Fox returned no SGF; will retry")?
            .as_bytes()
            .to_vec())
    }
}
