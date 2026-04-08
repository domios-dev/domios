//! SQLite schema, mem_items CRUD, blob storage. WAL mode for concurrent reads.
//! Schema is created on `init_schema` (idempotent — uses CREATE TABLE IF NOT EXISTS).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ids;
use super::MemStats;

const INLINE_BODY_LIMIT: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemKind {
    Skill,
    Fact,
    Note,
    Conversation,
    Script,
    Peer,
    Preference,
    Daily,
    Learning,
    Rule,
    Summary,
}

impl MemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Fact => "fact",
            Self::Note => "note",
            Self::Conversation => "conversation",
            Self::Script => "script",
            Self::Peer => "peer",
            Self::Preference => "preference",
            Self::Daily => "daily",
            Self::Learning => "learning",
            Self::Rule => "rule",
            Self::Summary => "summary",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "skill" => Self::Skill,
            "fact" => Self::Fact,
            "note" => Self::Note,
            "conversation" => Self::Conversation,
            "script" => Self::Script,
            "peer" => Self::Peer,
            "preference" => Self::Preference,
            "daily" => Self::Daily,
            "learning" => Self::Learning,
            "rule" => Self::Rule,
            "summary" => Self::Summary,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemScope {
    Global,
    Workspace(String),
}

impl MemScope {
    pub fn as_str(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Workspace(id) => format!("workspace:{}", id),
        }
    }

    pub fn from_str(s: &str) -> Self {
        if let Some(rest) = s.strip_prefix("workspace:") {
            Self::Workspace(rest.to_string())
        } else {
            Self::Global
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMemItem {
    pub kind: MemKind,
    pub scope: MemScope,
    pub title: String,
    pub summary: Option<String>,
    pub area: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub source: Option<String>,
    pub importance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemItem {
    pub id: String,
    pub uid: String,
    pub kind: String,
    pub scope: String,
    pub title: String,
    pub summary: Option<String>,
    pub area: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub source: Option<String>,
    pub importance: f64,
    pub created_at: String,
    pub updated_at: String,
    pub accessed_at: Option<String>,
}

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA synchronous = NORMAL;

        CREATE TABLE IF NOT EXISTS mem_items (
            id            TEXT PRIMARY KEY,
            uid           TEXT NOT NULL UNIQUE,
            kind          TEXT NOT NULL,
            scope         TEXT NOT NULL,
            title         TEXT NOT NULL,
            summary       TEXT,
            area          TEXT,
            tags_json     TEXT NOT NULL DEFAULT '[]',
            body_inline   TEXT,
            body_blob_path TEXT,
            source        TEXT,
            importance    REAL NOT NULL DEFAULT 3.0,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL,
            accessed_at   TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_mem_kind ON mem_items(kind);
        CREATE INDEX IF NOT EXISTS idx_mem_scope ON mem_items(scope);
        CREATE INDEX IF NOT EXISTS idx_mem_area ON mem_items(area);
        CREATE INDEX IF NOT EXISTS idx_mem_importance ON mem_items(importance DESC);
        CREATE INDEX IF NOT EXISTS idx_mem_created ON mem_items(created_at DESC);

        CREATE VIRTUAL TABLE IF NOT EXISTS mem_fts USING fts5(
            mem_id UNINDEXED,
            title,
            summary,
            body,
            tags,
            tokenize = 'unicode61'
        );

        CREATE TABLE IF NOT EXISTS entities (
            id            TEXT PRIMARY KEY,
            entity_type   TEXT NOT NULL,
            scope         TEXT NOT NULL,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);

        CREATE TABLE IF NOT EXISTS relations (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id         TEXT NOT NULL,
            relation_type   TEXT NOT NULL,
            to_id           TEXT NOT NULL,
            scope           TEXT NOT NULL,
            valid_from      TEXT NOT NULL,
            valid_to        TEXT,
            confidence      REAL NOT NULL DEFAULT 1.0,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at      TEXT NOT NULL,
            UNIQUE(from_id, relation_type, to_id, valid_from)
        );
        CREATE INDEX IF NOT EXISTS idx_relations_from ON relations(from_id);
        CREATE INDEX IF NOT EXISTS idx_relations_to ON relations(to_id);
        CREATE INDEX IF NOT EXISTS idx_relations_type ON relations(relation_type);
        CREATE INDEX IF NOT EXISTS idx_relations_valid ON relations(valid_from, valid_to);
        "#,
    )?;
    Ok(())
}

/// Insert a new mem_item, returning its slug id.
/// Body >2KB goes to a content-addressed blob; smaller bodies live inline.
/// Auto-resolves slug collisions by appending a hash suffix.
pub fn insert(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
) -> Result<String, String> {
    insert_with_options(conn, item, blobs_dir, false)
}

/// Insert with replace=true — UPDATE existing row with the same id instead
/// of generating a hash-suffixed alternative. Used by `mem save --replace`
/// for explicit upsert (e.g. when the user wants to overwrite the bundled
/// identity placeholder with their actual identity).
pub fn upsert(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
) -> Result<String, String> {
    insert_with_options(conn, item, blobs_dir, true)
}

fn insert_with_options(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
    replace: bool,
) -> Result<String, String> {
    let kind_str = item.kind.as_str();
    let slug = ids::slugify(&item.title);
    let candidate = ids::compose(kind_str, &slug);

    // Three modes:
    // - replace=true + exists → DELETE old + INSERT new with the same id (preserves id stability)
    // - replace=false + exists → switch to body-hashed suffix (collision-safe)
    // - new id → INSERT
    let collision = exists(conn, &candidate).map_err(|e| e.to_string())?;
    let id = if collision && replace {
        // Drop the existing row + its FTS mirror; we re-insert below.
        conn.execute("DELETE FROM mem_items WHERE id = ?1", params![&candidate])
            .map_err(|e| format!("delete on replace: {}", e))?;
        let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![&candidate]);
        candidate
    } else if collision {
        ids::with_collision_suffix(kind_str, &slug, &item.body)
    } else {
        candidate
    };

    let uid = ulid::Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let scope_str = item.scope.as_str();
    let tags_json = serde_json::to_string(&item.tags)
        .map_err(|e| format!("serialize tags: {}", e))?;

    let (body_inline, body_blob_path): (Option<String>, Option<String>) =
        if item.body.len() > INLINE_BODY_LIMIT {
            let path = write_blob(blobs_dir, &item.body)?;
            (None, Some(path))
        } else {
            (Some(item.body.clone()), None)
        };

    conn.execute(
        r#"INSERT INTO mem_items
           (id, uid, kind, scope, title, summary, area, tags_json,
            body_inline, body_blob_path, source, importance,
            created_at, updated_at, accessed_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL)"#,
        params![
            &id,
            &uid,
            kind_str,
            &scope_str,
            &item.title,
            &item.summary,
            &item.area,
            &tags_json,
            &body_inline,
            &body_blob_path,
            &item.source,
            item.importance,
            &now,
            &now,
        ],
    )
    .map_err(|e| format!("insert mem_item: {}", e))?;

    // Mirror into FTS5 (full body for search regardless of inline/blob)
    let tags_text = item.tags.join(" ");
    conn.execute(
        r#"INSERT INTO mem_fts (mem_id, title, summary, body, tags)
           VALUES (?1, ?2, ?3, ?4, ?5)"#,
        params![
            &id,
            &item.title,
            item.summary.as_deref().unwrap_or(""),
            &item.body,
            &tags_text,
        ],
    )
    .map_err(|e| format!("insert fts: {}", e))?;

    Ok(id)
}

pub fn exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mem_items WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn get(
    conn: &Connection,
    id: &str,
    blobs_dir: &Path,
) -> Result<Option<MemItem>, String> {
    let row: Option<(String, String, String, String, String, Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>, f64, String, String, Option<String>)> = conn
        .query_row(
            r#"SELECT id, uid, kind, scope, title, summary, area, tags_json,
                      body_inline, body_blob_path, source, importance,
                      created_at, updated_at, accessed_at
               FROM mem_items WHERE id = ?1"#,
            params![id],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                    r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
                    r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("query mem_item: {}", e))?;

    let Some((id, uid, kind, scope, title, summary, area, tags_json,
              body_inline, body_blob_path, source, importance,
              created_at, updated_at, accessed_at)) = row else {
        return Ok(None);
    };

    let body = if let Some(inline) = body_inline {
        inline
    } else if let Some(rel) = body_blob_path.as_ref() {
        read_blob(blobs_dir, rel)?
    } else {
        String::new()
    };

    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

    // Bump accessed_at (best-effort, ignore errors)
    let now = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "UPDATE mem_items SET accessed_at = ?1 WHERE id = ?2",
        params![&now, &id],
    );

    Ok(Some(MemItem {
        id, uid, kind, scope, title, summary, area, tags, body, source,
        importance, created_at, updated_at, accessed_at,
    }))
}

pub fn delete(conn: &Connection, id: &str) -> Result<bool, String> {
    let n = conn
        .execute("DELETE FROM mem_items WHERE id = ?1", params![id])
        .map_err(|e| format!("delete mem_item: {}", e))?;
    let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![id]);
    Ok(n > 0)
}

pub fn stats(conn: &Connection) -> Result<MemStats, String> {
    let total_items: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_items", [], |r| r.get(0))
        .map_err(|e| format!("count items: {}", e))?;
    let total_relations: i64 = conn
        .query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0))
        .map_err(|e| format!("count relations: {}", e))?;
    let total_entities: i64 = conn
        .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
        .map_err(|e| format!("count entities: {}", e))?;

    let mut by_kind: HashMap<String, u64> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT kind, COUNT(*) FROM mem_items GROUP BY kind")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (k, n) = row.map_err(|e| e.to_string())?;
        by_kind.insert(k, n as u64);
    }

    Ok(MemStats {
        total_items: total_items as u64,
        by_kind,
        total_relations: total_relations as u64,
        total_entities: total_entities as u64,
    })
}

// ── Blob storage (content-addressed) ────────────────────────────────────

fn write_blob(blobs_dir: &Path, body: &str) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let filename = format!("sha256_{}.txt", hash);
    let path: PathBuf = blobs_dir.join(&filename);
    if !path.exists() {
        std::fs::write(&path, body).map_err(|e| format!("write blob: {}", e))?;
    }
    Ok(filename)
}

fn read_blob(blobs_dir: &Path, filename: &str) -> Result<String, String> {
    let path = blobs_dir.join(filename);
    std::fs::read_to_string(&path).map_err(|e| format!("read blob {}: {}", filename, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (Connection, PathBuf) {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let tmp = std::env::temp_dir().join(format!("memstore-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&tmp).unwrap();
        (conn, tmp)
    }

    #[test]
    fn schema_initializes_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
    }

    #[test]
    fn insert_and_get_inline() {
        let (conn, blobs) = fresh();
        let id = insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "Test Fact".into(),
                summary: Some("a fact".into()),
                area: None,
                tags: vec!["a".into(), "b".into()],
                body: "short body".into(),
                source: None,
                importance: 3.0,
            },
            &blobs,
        )
        .unwrap();
        assert_eq!(id, "fact_test_fact");
        let got = get(&conn, &id, &blobs).unwrap().unwrap();
        assert_eq!(got.body, "short body");
        assert_eq!(got.tags, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn insert_blob_when_body_large() {
        let (conn, blobs) = fresh();
        let big = "x".repeat(5000);
        let id = insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Note,
                scope: MemScope::Global,
                title: "big".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: big.clone(),
                source: None,
                importance: 3.0,
            },
            &blobs,
        )
        .unwrap();
        let got = get(&conn, &id, &blobs).unwrap().unwrap();
        assert_eq!(got.body.len(), big.len());
    }

    #[test]
    fn upsert_replaces_existing_in_place() {
        let (conn, blobs) = fresh();
        let mk = |body: &str| NewMemItem {
            kind: MemKind::Preference,
            scope: MemScope::Global,
            title: "identity".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: body.into(),
            source: None,
            importance: 5.0,
        };
        let id1 = insert(&conn, &mk("placeholder"), &blobs).unwrap();
        assert_eq!(id1, "preference_identity");
        // Plain insert collides → hash suffix
        let id2 = insert(&conn, &mk("real identity"), &blobs).unwrap();
        assert_ne!(id1, id2);
        // Upsert collapses back to the canonical id
        let id3 = upsert(&conn, &mk("updated identity"), &blobs).unwrap();
        assert_eq!(id3, "preference_identity");
        let got = get(&conn, "preference_identity", &blobs).unwrap().unwrap();
        assert_eq!(got.body, "updated identity");
    }

    #[test]
    fn slug_collision_appends_hash() {
        let (conn, blobs) = fresh();
        let mk = |body: &str| NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "Same Title".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: body.into(),
            source: None,
            importance: 3.0,
        };
        let a = insert(&conn, &mk("body a"), &blobs).unwrap();
        let b = insert(&conn, &mk("body b"), &blobs).unwrap();
        assert_ne!(a, b);
        assert_eq!(a, "fact_same_title");
        assert!(b.starts_with("fact_same_title_"));
    }

    #[test]
    fn delete_removes_from_fts() {
        let (conn, blobs) = fresh();
        let id = insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Note,
                scope: MemScope::Global,
                title: "to delete".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "x".into(),
                source: None,
                importance: 3.0,
            },
            &blobs,
        )
        .unwrap();
        assert!(delete(&conn, &id).unwrap());
        assert!(get(&conn, &id, &blobs).unwrap().is_none());
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_fts WHERE mem_id = ?1",
                params![&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn stats_counts_by_kind() {
        let (conn, blobs) = fresh();
        for (i, kind) in [MemKind::Fact, MemKind::Fact, MemKind::Skill]
            .iter()
            .enumerate()
        {
            insert(
                &conn,
                &NewMemItem {
                    kind: *kind,
                    scope: MemScope::Global,
                    title: format!("item {}", i),
                    summary: None,
                    area: None,
                    tags: vec![],
                    body: "b".into(),
                    source: None,
                    importance: 3.0,
                },
                &blobs,
            )
            .unwrap();
        }
        let s = stats(&conn).unwrap();
        assert_eq!(s.total_items, 3);
        assert_eq!(s.by_kind.get("fact"), Some(&2));
        assert_eq!(s.by_kind.get("skill"), Some(&1));
    }
}
