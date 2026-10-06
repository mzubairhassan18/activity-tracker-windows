//! SQLite storage for activity segments.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::model::{ActivityState, Category, Segment};

/// Open database handle. Cheap enough to open per command; the sampler keeps
/// its own long-lived handle.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Directory for data files: %LOCALAPPDATA%\activity-tracker
    pub fn data_dir() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join("activity-tracker")
    }

    /// Default database path.
    pub fn db_path() -> PathBuf {
        Self::data_dir().join("tracker.db")
    }

    pub fn open() -> Result<Self> {
        Self::open_at(Self::db_path())
    }

    pub fn open_at(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating data dir {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        let _ = conn.query_row("PRAGMA journal_mode = WAL", [], |_r| Ok(()));
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;",
        )?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS segments (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                start_ms  INTEGER NOT NULL,
                end_ms    INTEGER NOT NULL,
                app       TEXT    NOT NULL,
                title     TEXT    NOT NULL,
                url       TEXT    NOT NULL DEFAULT '',
                category  TEXT    NOT NULL,
                state     TEXT    NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_segments_start ON segments(start_ms);
            CREATE INDEX IF NOT EXISTS idx_segments_end   ON segments(end_ms);
            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn insert_segments(&self, segments: &[Segment]) -> Result<usize> {
        if segments.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO segments (start_ms, end_ms, app, title, url, category, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for s in segments {
                stmt.execute(params![
                    s.start_ms,
                    s.end_ms,
                    s.app,
                    s.title,
                    s.url,
                    s.category.as_str(),
                    s.state.as_str(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(segments.len())
    }

    /// All segments overlapping [start_ms, end_ms), ordered by start time.
    pub fn segments(&self, start_ms: i64, end_ms: i64) -> Result<Vec<Segment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, start_ms, end_ms, app, title, url, category, state
             FROM segments
             WHERE start_ms < ?1 AND end_ms > ?2
             ORDER BY start_ms",
        )?;
        let rows = stmt.query_map(params![end_ms, start_ms], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, start_ms, end_ms, app, title, url, category, state) = r?;
            out.push(Segment {
                id,
                start_ms,
                end_ms,
                app,
                title,
                url,
                category: Category::parse(&category).unwrap_or(Category::Other),
                state: ActivityState::parse(&state).unwrap_or(ActivityState::Idle),
            });
        }
        Ok(out)
    }

    pub fn segment_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM segments", [], |r| r.get(0))?)
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Unix ms of the last recorded flush, if any (used by `status`).
    pub fn last_activity_ms(&self) -> Option<i64> {
        self.meta_get("last_flush_ms")
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
    }
}
