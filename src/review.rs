use crate::{Document, NodeId, Point, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    path::{Path, PathBuf},
};
mod cache;
pub struct Review {
    pub(crate) reviews: Connection,
    pub(crate) cache: Connection,
    pub(crate) document: Option<Document>,
    pub(crate) document_key: String,
    pub(crate) source: Option<PathBuf>,
}
pub(crate) fn connection(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(3))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}
fn same_file(source: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(source)
            .ok()
            .zip(fs::metadata(target).ok())
            .is_some_and(|(source, target)| {
                source.dev() == target.dev() && source.ino() == target.ino()
            })
    }
    #[cfg(not(unix))]
    {
        fs::canonicalize(source)
            .ok()
            .zip(fs::canonicalize(target).ok())
            .is_some_and(|(source, target)| source == target)
    }
}
impl Review {
    pub fn new(reviews: &Path, cache: &Path) -> Result<Self> {
        let reviews = connection(reviews)?;
        let cache = connection(cache)?;
        reviews.execute_batch("CREATE TABLE IF NOT EXISTS documents (id TEXT PRIMARY KEY, document TEXT NOT NULL); CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL); PRAGMA user_version=1;")?;
        cache.execute_batch("CREATE TABLE IF NOT EXISTS analysis (key TEXT PRIMARY KEY, visits INTEGER NOT NULL, result TEXT NOT NULL); CREATE TABLE IF NOT EXISTS training_games (key TEXT PRIMARY KEY, result TEXT NOT NULL); PRAGMA user_version=1;")?;
        cache.execute(
            "DELETE FROM analysis WHERE visits < ?1",
            [crate::analysis::MIN_CACHE_VISITS as i64],
        )?;
        Ok(Self {
            reviews,
            cache,
            document: None,
            document_key: String::new(),
            source: None,
        })
    }
    pub(crate) fn save_setting(&self, key: &str, value: &impl Serialize) -> Result<()> {
        self.reviews.execute("INSERT INTO settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, serde_json::to_string(value)?])?;
        Ok(())
    }
    pub(crate) fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let value: Option<String> = self
            .reviews
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .transpose()
    }
    pub fn import(&mut self, path: &Path) -> Result<()> {
        let (imported, source_komi) = Document::parse_with_source_komi(&fs::read(path)?)?;
        let key = imported.source_identity()?;
        let load = |key: &str| -> Result<Option<String>> {
            Ok(self
                .reviews
                .query_row("SELECT document FROM documents WHERE id=?1", [key], |r| {
                    r.get(0)
                })
                .optional()?)
        };
        let mut saved = load(&key)?;
        if saved.is_none() && source_komi != imported.komi {
            // Preserve variations/selection under the pre-fix identity. Only
            // review data migrates; analysis remains keyed by the actual komi.
            let mut legacy = imported.clone();
            legacy.komi = source_komi;
            saved = load(&legacy.source_identity()?)?;
        }
        let doc = if let Some(saved) = saved {
            let mut doc: Document = serde_json::from_str(&saved)?;
            doc.komi = imported.komi;
            for (id, node) in imported.nodes.iter().enumerate() {
                doc.nodes[id].props = node.props.clone();
            }
            doc
        } else {
            imported
        };
        self.save_document(&key, &doc)?;
        self.document = Some(doc);
        self.document_key = key;
        self.source = Some(fs::canonicalize(path)?);
        Ok(())
    }
    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }
    fn save_document(&self, key: &str, doc: &Document) -> Result<()> {
        self.reviews.execute("INSERT INTO documents(id,document) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET document=excluded.document",params![key,serde_json::to_string(doc)?])?;
        Ok(())
    }
    fn edit<T>(&mut self, edit: impl FnOnce(&mut Document) -> Result<T>) -> Result<T> {
        let mut doc = self.document.clone().ok_or("Open an SGF first")?;
        let result = edit(&mut doc)?;
        self.save_document(&self.document_key, &doc)?;
        self.document = Some(doc);
        Ok(result)
    }
    pub fn select(&mut self, id: NodeId) -> Result<()> {
        self.edit(|doc| {
            if id >= doc.nodes.len() {
                return Err("Unknown tree node".into());
            }
            doc.selected = id;
            Ok(())
        })
    }
    pub fn play(&mut self, point: Option<Point>) -> Result<NodeId> {
        self.edit(|doc| doc.append_move(point))
    }
    pub fn export(&self, path: &Path) -> Result<()> {
        if self
            .source
            .as_ref()
            .is_some_and(|source| same_file(source, path))
        {
            return Err("Choose a new file; export cannot overwrite the original SGF".into());
        }
        let doc = self.document.as_ref().ok_or("Open an SGF first")?;
        fs::write(path, doc.to_sgf())?;
        Ok(())
    }
}
