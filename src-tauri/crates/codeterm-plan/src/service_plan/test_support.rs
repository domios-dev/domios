use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::ports::{PlanIndexHit, PlanIndexQuery, PlanSearchPort};

#[cfg(test)]
mod tests;

static TEST_DATA_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();

pub(super) fn seed_plan_database(path: &Path) {
    use std::io::Write;

    if path.exists() {
        return;
    }
    static SCHEMA: OnceLock<Vec<u8>> = OnceLock::new();
    let schema = SCHEMA.get_or_init(|| {
        let template = tempfile::tempdir().expect("plan schema template directory");
        let path = template.path().join("plans.db");
        drop(
            super::store_db::PlanDatabase::open(path.clone()).expect("create plan schema template"),
        );
        std::fs::read(path).expect("read closed plan schema template")
    });
    let parent = path.parent().expect("plan fixture directory");
    std::fs::create_dir_all(parent).expect("create plan fixture directory");
    let mut copy = tempfile::NamedTempFile::new_in(parent).expect("plan schema copy");
    copy.write_all(schema).expect("write plan schema copy");
    match copy.persist_noclobber(path) {
        Ok(_) => {}
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => panic!("publish plan schema copy: {error}"),
    }
}

pub fn test_data_dir() -> PathBuf {
    TEST_DATA_DIR
        .get_or_init(|| tempfile::tempdir().expect("tempdir for test search port"))
        .path()
        .to_path_buf()
}

fn map_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlanIndexHit> {
    Ok(PlanIndexHit {
        score: 1.0,
        id: row.get(0)?,
        kind: row.get(1)?,
        scope: row.get(2)?,
        title: row.get(3)?,
        body: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        source: row.get(5)?,
        updated_at: row
            .get::<_, u64>(6)
            .map(|v| v.to_string())
            .unwrap_or_default(),
    })
}

pub struct TestSearchPort;

impl PlanSearchPort for TestSearchPort {
    fn data_dir(&self) -> PathBuf {
        test_data_dir()
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
                source TEXT,
                source_path TEXT,
                source_hash TEXT,
                source_mtime_ms INTEGER,
                importance REAL NOT NULL DEFAULT 1.0,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0,
                accessed_at INTEGER,
                is_pinned_wakeup INTEGER NOT NULL DEFAULT 0,
                author_kind TEXT NOT NULL DEFAULT 'system' CHECK(author_kind IN ('owner','neighbor','system')),
                author_uid TEXT,
                author_friendship TEXT,
                author_label TEXT DEFAULT 'unscoped'
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS mem_fts USING fts5(
                mem_id UNINDEXED,
                title,
                summary,
                body,
                tags
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
        conn: &Connection,
        q: &PlanIndexQuery,
        _blobs_dir: &Path,
    ) -> Result<Vec<PlanIndexHit>, String> {
        let limit = q.limit.unwrap_or(20) as i64;
        // Escape FTS5 special chars: wrap each term in double-quotes
        let fts_query = q
            .query
            .split_whitespace()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");
        let scope_clause = if q.scope.is_some() {
            " AND mi.scope = ?3"
        } else {
            ""
        };
        let sql = format!(
            "SELECT mi.id, mi.kind, mi.scope, mi.title, mi.body_inline, mi.source, mi.updated_at
             FROM mem_fts fts
             JOIN mem_items mi ON mi.id = fts.mem_id
             WHERE mem_fts MATCH ?1{scope_clause}
             ORDER BY fts.rank
             LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let hits = match &q.scope {
            Some(scope) => stmt
                .query_map(params![fts_query, limit, scope], map_hit)
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect(),
            None => stmt
                .query_map(params![fts_query, limit], map_hit)
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect(),
        };
        Ok(hits)
    }
}

#[cfg(test)]
pub(crate) fn task(id: &str) -> crate::plan::Task {
    serde_json::from_value(serde_json::json!({
        "id": id, "name": id, "type": "implement", "status": "pending"
    }))
    .expect("plain task fixture")
}
