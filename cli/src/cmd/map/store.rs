use rusqlite::{ffi::sqlite3_auto_extension, params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Once;

pub const SCHEMA_VERSION: i32 = 1;
pub const INDEXER_VERSION: &str = "0.1.0";

static VEC_EXT_REGISTERED: Once = Once::new();

fn register_sqlite_vec() {
    VEC_EXT_REGISTERED.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

pub struct Store {
    pub conn: Connection,
}

impl Store {
    pub fn open(workspace_root: &Path) -> rusqlite::Result<Store> {
        register_sqlite_vec();
        let dir = workspace_root.join(".codeterm").join("map").join("v1");
        std::fs::create_dir_all(&dir)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let path = dir.join("index.sqlite");
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Store { conn };
        store.init_schema(workspace_root)?;
        Ok(store)
    }

    pub fn db_path(workspace_root: &Path) -> PathBuf {
        workspace_root.join(".codeterm").join("map").join("v1").join("index.sqlite")
    }

    fn init_schema(&self, workspace_root: &Path) -> rusqlite::Result<()> {
        self.conn.execute_batch(SCHEMA_DDL)?;
        self.conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        self.set_meta("schema_version", &SCHEMA_VERSION.to_string())?;
        self.set_meta("indexer_version", INDEXER_VERSION)?;
        self.set_meta("workspace_cwd", &workspace_root.to_string_lossy())?;
        Ok(())
    }

    pub fn set_meta(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO map_meta(key,value) VALUES(?1,?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM map_meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
    }

    pub fn upsert_file(
        &self,
        rel_path: &str,
        language: Option<&str>,
        mtime_ns: i64,
        size: i64,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO file(path,language,mtime_ns,size,indexed,last_error) \
             VALUES(?1,?2,?3,?4,1,NULL) \
             ON CONFLICT(path) DO UPDATE SET \
                language = excluded.language, \
                mtime_ns = excluded.mtime_ns, \
                size     = excluded.size, \
                indexed  = 1, \
                last_error = NULL",
            params![rel_path, language, mtime_ns, size],
        )?;
        self.conn
            .query_row("SELECT id FROM file WHERE path = ?1", params![rel_path], |r| r.get(0))
    }

    pub fn file_id(&self, rel_path: &str) -> rusqlite::Result<Option<i64>> {
        self.conn
            .query_row("SELECT id FROM file WHERE path = ?1", params![rel_path], |r| r.get(0))
            .optional()
    }

    pub fn delete_file_data(&self, file_id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM occurrence WHERE file_id = ?1", params![file_id])?;
        self.conn.execute(
            "DELETE FROM edge WHERE source_node_id IN (SELECT id FROM node WHERE file_id = ?1) \
             OR target_node_id IN (SELECT id FROM node WHERE file_id = ?1)",
            params![file_id],
        )?;
        self.conn.execute("DELETE FROM node WHERE file_id = ?1", params![file_id])?;
        self.conn.execute("DELETE FROM tauri_bridge_signal WHERE file_id = ?1", params![file_id])?;
        self.conn.execute(
            "DELETE FROM chunk_vec WHERE chunk_id IN (SELECT id FROM chunk WHERE file_id = ?1)",
            params![file_id],
        )?;
        self.conn.execute("DELETE FROM chunk WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    pub fn insert_chunk(
        &self,
        file_id: i64,
        start_line: u32,
        end_line: u32,
        contents: &str,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO chunk(file_id,start_line,end_line,contents,embedded_at) \
             VALUES(?1,?2,?3,?4,NULL)",
            params![file_id, start_line as i64, end_line as i64, contents],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn store_chunk_vector(
        &self,
        chunk_id: i64,
        embedding: &[u8],
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM chunk_vec WHERE chunk_id = ?1",
            params![chunk_id],
        )?;
        self.conn.execute(
            "INSERT INTO chunk_vec(chunk_id,embedding) VALUES(?1,?2)",
            params![chunk_id, embedding],
        )?;
        let now = chrono::Utc::now().timestamp_millis();
        self.conn.execute(
            "UPDATE chunk SET embedded_at = ?1 WHERE id = ?2",
            params![now, chunk_id],
        )?;
        Ok(())
    }

    pub fn unembedded_chunks(&self) -> rusqlite::Result<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, contents FROM chunk WHERE embedded_at IS NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn vec_search(
        &self,
        query: &[u8],
        k: usize,
    ) -> rusqlite::Result<Vec<(i64, f64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, distance FROM chunk_vec \
             WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance",
        )?;
        let rows = stmt.query_map(params![query, k as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn chunk_to_node(&self, chunk_id: i64) -> rusqlite::Result<Option<ChunkLocator>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.file_id, c.start_line, c.end_line FROM chunk c WHERE c.id = ?1",
        )?;
        let row = stmt
            .query_row(params![chunk_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
            })
            .optional()?;
        let Some((file_id, start, _end)) = row else { return Ok(None) };

        let node_id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM node WHERE file_id = ?1 AND start_line IS NOT NULL \
                 AND start_line <= ?2 AND (end_line IS NULL OR end_line >= ?2) \
                 ORDER BY (?2 - start_line) ASC LIMIT 1",
                params![file_id, start],
                |r| r.get(0),
            )
            .optional()?;
        let node_id = if let Some(id) = node_id {
            Some(id)
        } else {
            self.conn
                .query_row(
                    "SELECT id FROM node WHERE file_id = ?1 AND kind = 'file'",
                    params![file_id],
                    |r| r.get(0),
                )
                .optional()?
        };
        Ok(node_id.map(|id| ChunkLocator { node_id: id, file_id, start_line: start }))
    }

    pub fn count_chunks(&self) -> rusqlite::Result<(i64, i64)> {
        let total: i64 = self.conn.query_row("SELECT COUNT(*) FROM chunk", [], |r| r.get(0))?;
        let embedded: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM chunk WHERE embedded_at IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
        Ok((total, embedded))
    }

    pub fn insert_bridge_signal(
        &self,
        file_id: i64,
        side: &str,
        command: &str,
        line: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO tauri_bridge_signal(file_id,side,command,line) VALUES(?1,?2,?3,?4)",
            params![file_id, side, command, line],
        )?;
        Ok(())
    }

    pub fn collect_bridge_signals(&self) -> rusqlite::Result<Vec<(i64, String, String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_id, side, command, line FROM tauri_bridge_signal",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn insert_node(&self, node: &NewNode) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO node(kind,serialized_name,display_name,file_id,start_line,start_col,end_line,end_col,metadata) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) \
             ON CONFLICT(serialized_name) DO UPDATE SET \
                kind = excluded.kind, \
                display_name = excluded.display_name, \
                file_id = excluded.file_id, \
                start_line = excluded.start_line, \
                start_col = excluded.start_col, \
                end_line = excluded.end_line, \
                end_col = excluded.end_col, \
                metadata = excluded.metadata",
            params![
                node.kind, node.serialized_name, node.display_name, node.file_id,
                node.start_line, node.start_col, node.end_line, node.end_col, node.metadata,
            ],
        )?;
        self.conn.query_row(
            "SELECT id FROM node WHERE serialized_name = ?1",
            params![node.serialized_name],
            |r| r.get(0),
        )
    }

    pub fn insert_edge(
        &self,
        kind: &str,
        source_node_id: i64,
        target_node_id: i64,
        weight: f64,
    ) -> rusqlite::Result<()> {
        self.insert_edge_with_metadata(kind, source_node_id, target_node_id, weight, None)
    }

    pub fn insert_edge_with_metadata(
        &self,
        kind: &str,
        source_node_id: i64,
        target_node_id: i64,
        weight: f64,
        metadata: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO edge(kind,source_node_id,target_node_id,weight,metadata) \
             VALUES(?1,?2,?3,?4,?5) \
             ON CONFLICT(kind,source_node_id,target_node_id) \
             DO UPDATE SET \
                weight   = edge.weight + excluded.weight, \
                metadata = COALESCE(excluded.metadata, edge.metadata)",
            params![kind, source_node_id, target_node_id, weight, metadata],
        )?;
        Ok(())
    }

    pub fn insert_occurrence(
        &self,
        file_id: i64,
        node_id: Option<i64>,
        symbol_name: &str,
        role: &str,
        line: i64,
        col: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO occurrence(file_id,node_id,symbol_name,role,line,col) \
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![file_id, node_id, symbol_name, role, line, col],
        )?;
        Ok(())
    }

    pub fn record_error(&self, file_id: Option<i64>, message: &str) -> rusqlite::Result<()> {
        let now = chrono::Utc::now().timestamp_millis();
        self.conn.execute(
            "INSERT INTO error(file_id,message,created_at) VALUES(?1,?2,?3)",
            params![file_id, message, now],
        )?;
        if let Some(fid) = file_id {
            self.conn.execute(
                "UPDATE file SET indexed = 0, last_error = ?1 WHERE id = ?2",
                params![message, fid],
            )?;
        }
        Ok(())
    }

    pub fn counts(&self) -> rusqlite::Result<Counts> {
        let files: i64 = self.conn.query_row("SELECT COUNT(*) FROM file", [], |r| r.get(0))?;
        let nodes: i64 = self.conn.query_row("SELECT COUNT(*) FROM node", [], |r| r.get(0))?;
        let edges: i64 = self.conn.query_row("SELECT COUNT(*) FROM edge", [], |r| r.get(0))?;
        let errors: i64 = self.conn.query_row("SELECT COUNT(*) FROM error", [], |r| r.get(0))?;
        Ok(Counts { files, nodes, edges, errors })
    }

    pub fn search_nodes(
        &self,
        query: &str,
        kind: Option<&str>,
        limit: u32,
    ) -> rusqlite::Result<Vec<NodeRow>> {
        let pattern = format!("%{}%", query);
        let mut stmt = match kind {
            Some(_) => self.conn.prepare(
                "SELECT n.kind, n.serialized_name, n.display_name, f.path, n.start_line \
                 FROM node n LEFT JOIN file f ON n.file_id = f.id \
                 WHERE n.display_name LIKE ?1 AND n.kind = ?2 \
                 ORDER BY n.display_name LIMIT ?3",
            )?,
            None => self.conn.prepare(
                "SELECT n.kind, n.serialized_name, n.display_name, f.path, n.start_line \
                 FROM node n LEFT JOIN file f ON n.file_id = f.id \
                 WHERE n.display_name LIKE ?1 \
                 ORDER BY n.display_name LIMIT ?2",
            )?,
        };
        let mut out = Vec::new();
        let collect = |row: &rusqlite::Row| -> rusqlite::Result<NodeRow> {
            Ok(NodeRow {
                kind: row.get(0)?,
                serialized_name: row.get(1)?,
                display_name: row.get(2)?,
                path: row.get::<_, Option<String>>(3)?,
                start_line: row.get::<_, Option<i64>>(4)?,
            })
        };
        match kind {
            Some(k) => {
                let rows = stmt.query_map(params![pattern, k, limit as i64], collect)?;
                for r in rows { out.push(r?); }
            }
            None => {
                let rows = stmt.query_map(params![pattern, limit as i64], collect)?;
                for r in rows { out.push(r?); }
            }
        }
        Ok(out)
    }
}

pub struct NewNode<'a> {
    pub kind: &'a str,
    pub serialized_name: &'a str,
    pub display_name: &'a str,
    pub file_id: Option<i64>,
    pub start_line: Option<i64>,
    pub start_col: Option<i64>,
    pub end_line: Option<i64>,
    pub end_col: Option<i64>,
    pub metadata: Option<&'a str>,
}

#[derive(Debug)]
pub struct Counts {
    pub files: i64,
    pub nodes: i64,
    pub edges: i64,
    pub errors: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct NodeRow {
    pub kind: String,
    pub serialized_name: String,
    pub display_name: String,
    pub path: Option<String>,
    pub start_line: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ChunkLocator {
    pub node_id: i64,
    pub file_id: i64,
    pub start_line: i64,
}

const SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS map_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS file (
  id        INTEGER PRIMARY KEY,
  path      TEXT NOT NULL UNIQUE,
  language  TEXT,
  mtime_ns  INTEGER NOT NULL,
  size      INTEGER NOT NULL,
  indexed   INTEGER NOT NULL DEFAULT 0,
  last_error TEXT
);

CREATE TABLE IF NOT EXISTS node (
  id              INTEGER PRIMARY KEY,
  kind            TEXT NOT NULL,
  serialized_name TEXT NOT NULL UNIQUE,
  display_name    TEXT NOT NULL,
  file_id         INTEGER REFERENCES file(id) ON DELETE CASCADE,
  start_line      INTEGER,
  start_col       INTEGER,
  end_line        INTEGER,
  end_col         INTEGER,
  metadata        TEXT
);
CREATE INDEX IF NOT EXISTS node_file_idx ON node(file_id);
CREATE INDEX IF NOT EXISTS node_kind_idx ON node(kind);
CREATE INDEX IF NOT EXISTS node_display_idx ON node(display_name);

CREATE TABLE IF NOT EXISTS edge (
  id             INTEGER PRIMARY KEY,
  kind           TEXT NOT NULL,
  source_node_id INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  target_node_id INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
  weight         REAL NOT NULL DEFAULT 1.0,
  metadata       TEXT,
  UNIQUE(kind, source_node_id, target_node_id)
);
CREATE INDEX IF NOT EXISTS edge_source_idx ON edge(source_node_id, kind);
CREATE INDEX IF NOT EXISTS edge_target_idx ON edge(target_node_id, kind);

CREATE TABLE IF NOT EXISTS occurrence (
  id          INTEGER PRIMARY KEY,
  file_id     INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  node_id     INTEGER REFERENCES node(id) ON DELETE SET NULL,
  symbol_name TEXT NOT NULL,
  role        TEXT NOT NULL,
  line        INTEGER NOT NULL,
  col         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS occurrence_file_idx ON occurrence(file_id);
CREATE INDEX IF NOT EXISTS occurrence_name_idx ON occurrence(symbol_name);

CREATE TABLE IF NOT EXISTS error (
  id        INTEGER PRIMARY KEY,
  file_id   INTEGER REFERENCES file(id) ON DELETE CASCADE,
  message   TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tauri_bridge_signal (
  id        INTEGER PRIMARY KEY,
  file_id   INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  side      TEXT NOT NULL,
  command   TEXT NOT NULL,
  line      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS tauri_bridge_signal_command_idx ON tauri_bridge_signal(command);
CREATE INDEX IF NOT EXISTS tauri_bridge_signal_file_idx ON tauri_bridge_signal(file_id);

CREATE TABLE IF NOT EXISTS chunk (
  id          INTEGER PRIMARY KEY,
  file_id     INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  start_line  INTEGER NOT NULL,
  end_line    INTEGER NOT NULL,
  contents    TEXT NOT NULL,
  embedded_at INTEGER
);
CREATE INDEX IF NOT EXISTS chunk_file_idx ON chunk(file_id);

CREATE VIRTUAL TABLE IF NOT EXISTS chunk_vec USING vec0(
  chunk_id INTEGER PRIMARY KEY,
  embedding FLOAT[384]
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let fid = store.upsert_file("a/b.rs", Some("rust"), 1234, 99).unwrap();
        assert_eq!(store.file_id("a/b.rs").unwrap(), Some(fid));

        let nid = store.insert_node(&NewNode {
            kind: "function",
            serialized_name: "rust:a/b.rs::greet",
            display_name: "greet",
            file_id: Some(fid),
            start_line: Some(0),
            start_col: Some(0),
            end_line: Some(0),
            end_col: Some(10),
            metadata: None,
        }).unwrap();

        let nid_file = store.insert_node(&NewNode {
            kind: "file",
            serialized_name: "file:a/b.rs",
            display_name: "b.rs",
            file_id: Some(fid),
            start_line: None, start_col: None, end_line: None, end_col: None, metadata: None,
        }).unwrap();

        store.insert_edge("contain", nid_file, nid, 1.0).unwrap();
        store.insert_occurrence(fid, Some(nid), "greet", "def", 0, 0).unwrap();

        let counts = store.counts().unwrap();
        assert_eq!(counts.files, 1);
        assert_eq!(counts.nodes, 2);
        assert_eq!(counts.edges, 1);

        let rows = store.search_nodes("greet", None, 10).unwrap();
        assert!(rows.iter().any(|r| r.display_name == "greet"));
    }
}
