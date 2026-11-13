#![allow(dead_code)]
use katastro::{
    Result,
    pandanet::{Credentials, Service},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    thread,
};
use tiny_http::{Header, Response, Server};

#[derive(Default)]
pub struct Vault(pub Mutex<HashMap<String, String>>);
impl Credentials for Vault {
    fn load(&self, account: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn save(&self, account: &str, password: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(account.into(), password.into());
        Ok(())
    }
    fn forget(&self, account: &str) -> Result<()> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Request {
    pub url: String,
    pub body: String,
}
pub struct Fixture {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
    server: Arc<Server>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    pub fn new(handler: impl Fn(Request) -> (u16, String) + Send + 'static) -> Self {
        let server = Arc::new(Server::http("127.0.0.1:0").unwrap());
        let endpoint = format!("http://{}/cgi-bin/cgi.exe?MH", server.server_addr());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let listener = server.clone();
        let thread = thread::spawn(move || {
            while let Ok(mut req) = listener.recv() {
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).unwrap();
                let request = Request {
                    url: req.url().into(),
                    body,
                };
                recorded.lock().unwrap().push(request.clone());
                let (code, body) = handler(request);
                let _ = req.respond(
                    Response::from_string(body)
                        .with_status_code(code)
                        .with_header(
                            Header::from_bytes("Content-Type", "text/html; charset=UTF-8").unwrap(),
                        ),
                );
            }
        });
        Self {
            endpoint,
            requests,
            server,
            thread: Some(thread),
        }
    }
    pub fn service(&self, root: &Path, vault: Arc<Vault>) -> Service {
        let mut service = Service::open(root, &self.endpoint, vault).unwrap();
        service.set_request_interval(std::time::Duration::ZERO);
        service
    }
    pub fn downloads(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.url.contains("mode=sgf"))
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.unblock();
        self.thread.take().unwrap().join().unwrap();
    }
}
pub fn page(current: usize, total: usize, games: &[usize]) -> String {
    let rows=games.iter().map(|id|format!(r#"<tr><td>O</td><td>2026/11/{id:02} 12:00</td><td>example 6k+</td><td>opponent 5k+</td><td>9</td><td>-</td><td>6.5</td><td>White won by resign</td><td><a href="/cgi-bin/cgi.exe?MHkey=private-session&amp;pg=DownLoadUGI&amp;v1=game{id}&amp;v2=record&amp;mode=sgf">SGF</a></td><td>Write journal</td></tr>"#)).collect::<String>();
    let nav = (1..=total)
        .map(|p| {
            if p == current {
                format!("<td>{p}</td>")
            } else {
                format!("<td><a href=\"javascript:ShowPage('{p}')\">{p}</a></td>")
            }
        })
        .collect::<String>();
    format!(
        r#"<!doctype html><meta charset="UTF-8"><title>Let's make friends! Net social plaza - [ Search Result ]</title><form><input name="key" value="private-session"><input name="MyName" value="example"></form><table><tr><th>[ W/L ]</th><th>[ Date ]</th><th>[ White ]</th><th>[ Black ]</th><th>[ Size ]</th><th>[ H ]</th><th>[ Komi ]</th><th>[ Result ]</th><th>[ View ]</th></tr>{rows}</table><table class="backnext"><tr>{nav}</tr></table>"#
    )
}
pub fn sgf(id: usize) -> String {
    format!("(;FF[4]GM[1]SZ[9]KM[6.5]PB[opponent]PW[example]DT[2026-11-{id:02}];B[cc];W[gg];B[cg])")
}
