use rusqlite::Connection;
use std::path::{Path, PathBuf};

use super::ports::{PlanIndexHit, PlanIndexQuery, PlanSearchPort};

pub struct TestSearchPort {
    root: PathBuf,
}

impl TestSearchPort {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }
}

impl PlanSearchPort for TestSearchPort {
    fn data_dir(&self) -> PathBuf {
        self.root.clone()
    }

    fn ensure_embedding_registered(&self) {}

    fn init_embedding_ext(&self, _conn: &Connection) {}

    fn init_schema(&self, conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS mem_items (
                id TEXT PRIMARY KEY,
                uid TEXT NOT NULL UNIQUE DEFAULT '',
                kind TEXT NOT NULL DEFAULT '',
                scope TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                summary TEXT,
                area TEXT,
                tags_json TEXT NOT NULL DEFAULT '[]',
                body_inline TEXT,
                body_blob_path TEXT,
                source_path TEXT,
                source_hash TEXT,
                source_mtime_ms INTEGER,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS mem_fts (
                mem_id TEXT,
                title TEXT,
                summary TEXT,
                body TEXT,
                tags TEXT
            );
            CREATE TABLE IF NOT EXISTS mem_vectors (
                mem_id TEXT,
                chunk_index INTEGER,
                embedding BLOB
            );
            "#,
        )
        .map_err(|e| e.to_string())
    }

    fn embed(&self, _conn: &Connection, _id: &str, _body: &str) -> Result<(), String> {
        Ok(())
    }

    fn run_search(
        &self,
        _conn: &Connection,
        _q: &PlanIndexQuery,
        _blobs_dir: &Path,
    ) -> Result<Vec<PlanIndexHit>, String> {
        Ok(vec![])
    }
}
