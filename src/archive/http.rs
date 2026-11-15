//! Shared transport limits and sanitized errors; redirects stay provider-specific.
use super::Server;
use crate::Result;
use reqwest::{
    Url,
    blocking::{Client, Response},
    redirect::Policy,
};
use std::{io::Read, time::Duration};

#[derive(Clone)]
pub(super) struct Http {
    client: Client,
    server: Server,
}
impl Http {
    pub fn new(server: Server) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .connect_timeout(Duration::from_secs(10))
                .redirect(Policy::none())
                .referer(false)
                .cookie_store(server == Server::Pandanet)
                .user_agent("Katastro/0.1 (personal game archive sync)")
                .build()?,
            server,
        })
    }
    pub fn send(&self, url: Url, form: Option<&[(&str, &str)]>) -> Result<Response> {
        let request = match form {
            Some(fields) => self.client.post(url).form(fields),
            None => self.client.get(url),
        };
        request.send().map_err(|error| {
            // reqwest errors can expose session-bearing URLs. Never surface them.
            if self.server == Server::Pandanet && error.is_timeout() {
                "Pandanet request timed out; sync will retry".into()
            } else {
                format!(
                    "Could not connect to {}; sync will retry",
                    self.server.label()
                )
                .into()
            }
        })
    }
    pub fn read(&self, response: Response) -> Result<Vec<u8>> {
        let server = self.server.label();
        if !response.status().is_success() {
            return Err(format!(
                "{server} returned HTTP {}; sync will retry",
                response.status().as_u16()
            )
            .into());
        }
        const LIMIT: u64 = 16 * 1024 * 1024;
        let mut bytes = Vec::new();
        response
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| format!("Could not read {server} response"))?;
        if bytes.len() as u64 > LIMIT {
            return Err(format!("{server} response exceeds the download limit").into());
        }
        Ok(bytes)
    }
}
