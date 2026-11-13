use super::{Cancellation, Game};
use crate::{Result, digest};
use reqwest::{
    Url,
    blocking::{Client, Response},
    redirect::Policy,
};
use scraper::{ElementRef, Html, Selector};
use std::{
    collections::BTreeMap,
    io::Read,
    path::PathBuf,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

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
pub(super) struct Remote {
    endpoint: Url,
    client: Client,
    session: Option<Zeroizing<String>>,
    pub interval: Duration,
    last_request: Option<Instant>,
}
fn select(s: &str) -> Selector {
    Selector::parse(s).expect("static HTML selector")
}
fn text(e: ElementRef<'_>) -> String {
    e.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn network_error(error: reqwest::Error) -> Box<dyn std::error::Error + Send + Sync> {
    // reqwest errors can contain session-bearing URLs. Never surface those.
    if error.is_timeout() {
        "Pandanet request timed out; sync will retry".into()
    } else {
        "Could not connect to Pandanet; sync will retry".into()
    }
}
impl Remote {
    pub fn new(endpoint: &str) -> Result<Self> {
        let endpoint = Url::parse(endpoint)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::none())
            .referer(false)
            .cookie_store(true)
            .user_agent("Katastro/0.1 (personal game archive sync)")
            .build()?;
        let remote = Self {
            endpoint,
            client,
            session: None,
            interval: Duration::ZERO,
            last_request: None,
        };
        remote.allowed(&remote.endpoint)?;
        Ok(remote)
    }
    pub fn reset(&mut self) {
        self.session = None;
    }
    fn allowed(&self, url: &Url) -> Result<()> {
        let local =
            self.endpoint.host_str() == Some("127.0.0.1") && url.origin() == self.endpoint.origin();
        let pandanet = url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|h| h == "pandanet.co.jp" || h.ends_with(".pandanet.co.jp"));
        if (local || pandanet) && url.username().is_empty() && url.password().is_none() {
            Ok(())
        } else {
            Err("Pandanet returned an unsupported URL; no credentials were sent to it".into())
        }
    }
    fn request(
        &mut self,
        mut url: Url,
        mut form: Option<&[(&str, &str)]>,
        cancel: &Cancellation,
    ) -> Result<(Url, Vec<u8>, String)> {
        for _ in 0..5 {
            self.allowed(&url)?;
            cancel.check()?;
            if let Some(last) = self.last_request {
                cancel.wait(self.interval.saturating_sub(last.elapsed()))?;
            }
            self.last_request = Some(Instant::now());
            let response = if let Some(fields) = form {
                self.client.post(url.clone()).form(fields).send()
            } else {
                self.client.get(url.clone()).send()
            }
            .map_err(network_error)?;
            cancel.check()?;
            if response.status().is_redirection() {
                if !matches!(response.status().as_u16(), 301..=303) {
                    return Err("Unsupported Pandanet redirect".into());
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|x| x.to_str().ok())
                    .ok_or("Missing Pandanet redirect location")?;
                url = url
                    .join(location)
                    .map_err(|_| "Invalid Pandanet redirect")?;
                form = None;
                continue;
            }
            if !response.status().is_success() {
                return Err(format!(
                    "Pandanet returned HTTP {}; sync will retry",
                    response.status().as_u16()
                )
                .into());
            }
            let encoding = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|x| x.to_str().ok())
                .unwrap_or("")
                .to_owned();
            return Ok((url, read_limited(response)?, encoding));
        }
        Err("Too many Pandanet redirects".into())
    }
    pub fn page(
        &mut self,
        username: &str,
        password: &str,
        page: usize,
        cancel: &Cancellation,
    ) -> Result<Page> {
        let number = page.to_string();
        let mut form = vec![
            ("pg", "SearchResult"),
            ("MyName", username),
            ("PageNo", number.as_str()),
            ("PerPage", "100"),
        ];
        let session = self.session.clone();
        if let Some(key) = &session {
            form.push(("key", key.as_str()));
        } else {
            form.extend([("userid", username), ("password", password)]);
        }
        let (mut url, mut bytes, mut encoding) =
            self.request(self.endpoint.clone(), Some(&form), cancel)?;
        for _ in 0..4 {
            let html = decode(&bytes, &encoding);
            let doc = Html::parse_document(&html);
            if doc.select(&select("input")).any(|e| {
                e.value()
                    .attr("type")
                    .is_some_and(|t| t.eq_ignore_ascii_case("password"))
            }) {
                return Err("Pandanet sign-in required. Check your login or complete your Net Social Plaza profile.".into());
            }
            let refresh = doc
                .select(&select("meta[http-equiv]"))
                .find(|e| {
                    e.value()
                        .attr("http-equiv")
                        .is_some_and(|v| v.eq_ignore_ascii_case("refresh"))
                })
                .and_then(|e| e.value().attr("content"))
                .and_then(|s| s.split_once(';'))
                .and_then(|(_, s)| s.trim().split_once('='))
                .filter(|(k, _)| k.trim().eq_ignore_ascii_case("url"))
                .map(|(_, v)| v.trim().trim_matches(['\'', '"']).to_owned());
            if let Some(target) = refresh {
                let target = url.join(&target).map_err(|_| "Invalid Pandanet redirect")?;
                (url, bytes, encoding) = self.request(target, None, cancel)?;
                continue;
            }
            let parsed = parse_page(&doc, &url, username, page)?;
            if let Some(key) = doc
                .select(&select("input[name=key]"))
                .next()
                .and_then(|e| e.value().attr("value"))
            {
                self.session = Some(Zeroizing::new(key.into()));
            }
            for entry in &parsed.entries {
                self.allowed(&entry.url)?;
            }
            return Ok(parsed);
        }
        Err("Pandanet profile setup or repeated redirect; open Net Social Plaza to finish signing in".into())
    }
    pub fn download(&mut self, url: Url, cancel: &Cancellation) -> Result<Vec<u8>> {
        Ok(self.request(url, None, cancel)?.1)
    }
}
fn read_limited(response: Response) -> Result<Vec<u8>> {
    const LIMIT: u64 = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    response
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read Pandanet response")?;
    if bytes.len() as u64 > LIMIT {
        return Err("Pandanet response exceeds the download limit".into());
    }
    Ok(bytes)
}
fn decode<'a>(bytes: &'a [u8], content_type: &str) -> std::borrow::Cow<'a, str> {
    let label = content_type
        .split(';')
        .find_map(|s| s.trim().strip_prefix("charset="))
        .map(|s| s.trim_matches('"'));
    if let Some(enc) = label.and_then(|s| encoding_rs::Encoding::for_label(s.as_bytes())) {
        return enc.decode(bytes).0;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.into(),
        Err(_) => encoding_rs::SHIFT_JIS.decode(bytes).0,
    }
}
fn parse_page(doc: &Html, base: &Url, username: &str, requested: usize) -> Result<Page> {
    let mut entries = Vec::new();
    let mut recognized = false;
    for row in doc.select(&select("tr")) {
        let cells: Vec<_> = row
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|e| matches!(e.value().name(), "td" | "th"))
            .collect();
        if cells.len() < 9 {
            continue;
        }
        let values: Vec<_> = cells.iter().copied().map(text).collect();
        if values[1].contains("Date") && values[2].contains("White") && values[3].contains("Black")
        {
            recognized = true;
            continue;
        }
        let sgf = cells[8]
            .select(&select("a[href]"))
            .find(|a| text(*a).eq_ignore_ascii_case("SGF"));
        let Some(sgf) = sgf else {
            continue;
        };
        if ![&values[2], &values[3]].iter().any(|name| {
            name.split_whitespace()
                .next()
                .unwrap_or("")
                .split(':')
                .any(|n| n.eq_ignore_ascii_case(username))
        }) {
            return Err("Pandanet returned another player's archive; sync stopped".into());
        }
        let url = base
            .join(sgf.value().attr("href").unwrap())
            .map_err(|_| "Invalid Pandanet SGF link")?;
        let params: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        let v1 = params
            .get("v1")
            .filter(|v| !v.is_empty())
            .ok_or("Pandanet SGF link has no game identity")?;
        let v2 = params
            .get("v2")
            .ok_or("Pandanet SGF link has no record identity")?;
        let id = digest(format!("{v1}\0{v2}").as_bytes());
        entries.push(Entry {
            game: Game {
                id,
                date: values[1].clone(),
                white: values[2].clone(),
                black: values[3].clone(),
                result: values[7].clone(),
                path: PathBuf::new(),
            },
            url,
        });
    }
    if !recognized {
        return Err("Could not read Pandanet's game list; the website may have changed or require profile setup".into());
    }
    let mut total = 1;
    let mut current = None;
    for cell in doc.select(&select("table.backnext td")) {
        if let Ok(page) = text(cell).parse::<usize>() {
            total = total.max(page);
            if cell.select(&select("a")).next().is_none() {
                current = Some(page);
            }
        }
    }
    if total > 10000 || current.unwrap_or(1) != requested {
        return Err("Pandanet returned unexpected archive pagination".into());
    }
    if entries.is_empty() && total > 1 {
        return Err("Pandanet returned an empty archive page unexpectedly".into());
    }
    Ok(Page { entries, total })
}
