//! SQLite state: chats, runs, drafts, the delivery outbox and transcript
//! items (specification sections 5 and 6).
//!
//! Storage depends on core only, never on providers, and never starts work.
//! One [`worker::Worker`] thread serializes every write and bounded read.

mod codec;
pub mod repository;
pub mod worker;

use std::path::{Path, PathBuf};

use rusqlite::Connection;

pub use worker::Worker;

/// The schema version this build reads and writes.
pub const SCHEMA_VERSION: i64 = 1;
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_initial.sql")];

#[derive(Debug)]
pub enum StoreError {
    /// The database was written by a newer Bukno. It is left untouched.
    NewerSchema {
        found: i64,
        supported: i64,
    },
    /// The integrity check failed. The original files are preserved.
    Corrupt(String),
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewerSchema { found, supported } => write!(
                f,
                "the database was written by a newer Bukno (schema {found}; this version understands {supported})"
            ),
            Self::Corrupt(detail) => write!(f, "the database failed its integrity check: {detail}"),
            Self::Sqlite(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// The open database.
pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Open or create `state.sqlite` in `state_dir`, migrating it if needed.
    pub fn open(state_dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(state_dir)?;
        let path = state_dir.join("state.sqlite");
        let conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(StoreError::Corrupt(check));
        }
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::NewerSchema { found: version, supported: SCHEMA_VERSION });
        }
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let mut store = Self { conn, path };
        if version < SCHEMA_VERSION {
            if version > 0 {
                store.backup(state_dir, version)?;
            }
            store.migrate(version)?;
        }
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Copy the database with SQLite's backup mechanism before a migration.
    fn backup(&self, state_dir: &Path, version: i64) -> Result<PathBuf, StoreError> {
        let dir = state_dir.join("backups");
        std::fs::create_dir_all(&dir)?;
        let stamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
        let target = dir.join(format!("state-schema{version}-{stamp}.sqlite"));
        let mut copy = Connection::open(&target)?;
        {
            let backup = rusqlite::backup::Backup::new(&self.conn, &mut copy)?;
            backup.run_to_completion(256, std::time::Duration::from_millis(5), None)?;
        }
        let check: String = copy.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(StoreError::Corrupt(format!("backup check failed: {check}")));
        }
        Ok(target)
    }

    fn migrate(&mut self, from: i64) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(from as usize) {
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Fold the write-ahead log back into the database, for an orderly quit.
    pub fn checkpoint(&self) -> Result<(), StoreError> {
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    pub fn connection(&mut self) -> &mut Connection {
        &mut self.conn
    }
}
