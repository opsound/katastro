//! Archive metadata, credentials preferences, and immutable downloaded originals.
use super::*;
impl Service {
    pub fn open(root: &Path, endpoint: &str, credentials: Arc<dyn Credentials>) -> Result<Self> {
        Self::open_for(root, endpoint, credentials, Server::Pandanet)
    }
    pub fn open_for(
        root: &Path,
        endpoint: &str,
        credentials: Arc<dyn Credentials>,
        server: Server,
    ) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        }
        let db = crate::review::connection(&root.join("sync.sqlite"))?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS config (singleton INTEGER PRIMARY KEY CHECK(singleton=1),username TEXT NOT NULL,remember INTEGER NOT NULL,automatic INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS accounts (username TEXT PRIMARY KEY,last_sync INTEGER,last_full INTEGER);
            CREATE TABLE IF NOT EXISTS games (username TEXT NOT NULL,id TEXT NOT NULL,date TEXT NOT NULL,white TEXT NOT NULL,black TEXT NOT NULL,result TEXT NOT NULL,filename TEXT NOT NULL,sha TEXT NOT NULL,PRIMARY KEY(username,id));
            CREATE TABLE IF NOT EXISTS failures (username TEXT NOT NULL,id TEXT NOT NULL,date TEXT NOT NULL,message TEXT NOT NULL,PRIMARY KEY(username,id));")?;
        let has_limit = db
            .prepare("PRAGMA table_info(config)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == "max_games");
        if !has_limit {
            db.execute(
                "ALTER TABLE config ADD COLUMN max_games INTEGER DEFAULT 200",
                [],
            )?;
        }
        let saved = db
            .query_row(
                "SELECT username,remember,automatic,max_games FROM config WHERE singleton=1",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, bool>(1)?,
                        r.get::<_, bool>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;
        let mut state = Snapshot {
            server,
            status: format!("Connect your {} account to sync games", server.label()),
            limit: Some(200),
            ..Snapshot::default()
        };
        if let Some((username, remember, automatic, limit)) = saved {
            state.limit = limit.map(|n| n.max(1) as usize);
            state.username = username;
            state.remember = remember;
            state.automatic = automatic;
            state.status = if automatic {
                "Waiting to sync"
            } else {
                "Automatic sync paused"
            }
            .into();
        }
        let mut service = Self {
            state,
            root: root.into(),
            db,
            remote: remote::Remote::new(endpoint, server)?,
            credentials,
            password: None,
            needs_login: false,
            next_due: 0,
            last_full: None,
            remember_pending: false,
            parallel_downloads: 2,
        };
        service.load_account()?;
        Ok(service)
    }
    pub(super) fn load_account(&mut self) -> Result<()> {
        let times = self
            .db
            .query_row(
                "SELECT last_sync,last_full FROM accounts WHERE username=?1",
                [&self.state.username],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
            .unwrap_or((None, None));
        self.state.failures = self
            .db
            .prepare("SELECT id,date,message FROM failures WHERE username=?1 ORDER BY date DESC")?
            .query_map([&self.state.username], |r| {
                Ok(FailedGame {
                    id: r.get(0)?,
                    date: r.get(1)?,
                    message: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        self.state.last_sync = times.0.map(|t| t as u64);
        self.last_full = times.1.map(|t| t as u64);
        let mut stmt=self.db.prepare("SELECT id,date,white,black,result,filename FROM games WHERE username=?1 ORDER BY date DESC,id")?;
        self.state.games = stmt
            .query_map([&self.state.username], |r| {
                Ok(Game {
                    id: r.get(0)?,
                    date: r.get(1)?,
                    white: r.get(2)?,
                    black: r.get(3)?,
                    result: r.get(4)?,
                    path: self.root.join(r.get::<_, String>(5)?),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(())
    }
    pub(super) fn save_config(&self) -> Result<()> {
        self.db.execute("INSERT INTO config (singleton,username,remember,automatic,max_games) VALUES (1,?1,?2,?3,?4) ON CONFLICT(singleton) DO UPDATE SET username=excluded.username,remember=excluded.remember,automatic=excluded.automatic,max_games=excluded.max_games",params![self.state.username,self.state.remember,self.state.automatic,self.state.limit.map(|n|n as i64)])?;
        Ok(())
    }
    pub(super) fn save_download(&mut self, mut game: Game, bytes: &[u8]) -> Result<()> {
        if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'<') {
            return Err(format!(
                "{} returned an HTML page instead of an SGF; will retry",
                self.state.server.label()
            )
            .into());
        }
        let document =
            crate::Document::parse(bytes).map_err(|e| format!("Could not import this SGF: {e}"))?;
        if self.state.server == Server::Fox {
            let root = &document.nodes[0];
            let player = |name, rank| {
                let name = root.property(name).unwrap_or("");
                let rank = root
                    .property(rank)
                    .unwrap_or("")
                    .replace('级', "k")
                    .replace('段', "d");
                format!("{name} {rank}").trim().to_owned()
            };
            game.black = player("PB", "BR");
            game.white = player("PW", "WR");
            game.result = root.property("RE").unwrap_or("").into();
        }
        self.store(game, bytes)
    }
    pub(super) fn store(&mut self, game: Game, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        let folder = PathBuf::from("games").join(crate::digest(self.state.username.as_bytes()));
        std::fs::create_dir_all(self.root.join(&folder))?;
        let filename = folder.join(format!("{}.sgf", game.id));
        let target = self.root.join(&filename);
        let hash = crate::digest(bytes);
        if target.exists() {
            if crate::digest(&std::fs::read(&target)?) != hash {
                return Err(
                    "A downloaded original has changed locally; it was not overwritten".into(),
                );
            }
        } else {
            let partial = target.with_extension("part");
            // Recover only our own unfinished transfer, never an SGF original.
            if partial.exists() {
                std::fs::remove_file(&partial)?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&partial)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            std::fs::hard_link(&partial, &target)?;
            std::fs::remove_file(&partial)?;
        }
        self.db.execute("INSERT INTO games VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(username,id) DO NOTHING",params![self.state.username,game.id,game.date,game.white,game.black,game.result,filename.to_string_lossy(),hash])?;
        self.db.execute(
            "DELETE FROM failures WHERE username=?1 AND id=?2",
            params![self.state.username, game.id],
        )?;
        self.load_account()?;
        Ok(())
    }
}
