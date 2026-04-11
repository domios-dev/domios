//! mem_items SQLite schema + CRUD + blob store.

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
    #[serde(default)]
    pub access_count: i64,
    #[serde(default)]
    pub recurrence_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_y: Option<f64>,
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
            accessed_at   TEXT,
            access_count  INTEGER NOT NULL DEFAULT 0,
            recurrence_count INTEGER NOT NULL DEFAULT 0
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

        CREATE TABLE IF NOT EXISTS entity_aliases (
            alias_id     TEXT PRIMARY KEY,
            canonical_id TEXT NOT NULL,
            created_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_aliases_canonical ON entity_aliases(canonical_id);
        "#,
    )?;

    let has_graph_x: bool = {
        let mut stmt = conn.prepare("PRAGMA table_info(mem_items)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        cols.iter().any(|c| c == "graph_x")
    };
    if !has_graph_x {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN graph_x REAL;\n\
             ALTER TABLE mem_items ADD COLUMN graph_y REAL;",
        )?;
    }

    let has_access_count = {
        let mut stmt = conn.prepare("PRAGMA table_info(mem_items)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        cols.iter().any(|c| c == "access_count")
    };
    if !has_access_count {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    let has_recurrence = {
        let mut stmt = conn.prepare("PRAGMA table_info(mem_items)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        cols.iter().any(|c| c == "recurrence_count")
    };
    if !has_recurrence {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN recurrence_count INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    Ok(())
}

/// Startup repair pass: drops bogus entities, bad time windows, orphans.
pub fn repair_inconsistencies(conn: &Connection) -> Result<(), String> {
    let bogus_entities = conn.execute(
        r#"DELETE FROM entities
           WHERE id NOT LIKE 'person_%'
             AND id NOT LIKE 'project_%'"#,
        [],
    ).map_err(|e| format!("repair entities: {}", e))? as u64;

    let bogus_relations = conn.execute(
        r#"DELETE FROM relations
           WHERE (from_id NOT LIKE 'person_%' AND from_id NOT LIKE 'project_%' AND from_id NOT IN (SELECT id FROM mem_items))
              OR (to_id NOT LIKE 'person_%' AND to_id NOT LIKE 'project_%' AND to_id NOT IN (SELECT id FROM mem_items))
              OR relation_type = 'kind_of'
              OR relation_type = 'belongs_to'
              OR relation_type = 'belongs_to_category'
              OR relation_type = 'is_a'"#,
        [],
    ).map_err(|e| format!("repair relations: {}", e))? as u64;

    let bad_temporal = conn.execute(
        "DELETE FROM relations WHERE valid_to IS NOT NULL AND valid_to < valid_from",
        [],
    ).map_err(|e| format!("repair temporal: {}", e))? as u64;

    let empty_titles = conn.execute(
        "UPDATE mem_items SET title = 'Untitled ' || substr(uid, 1, 8) WHERE title = '' OR title IS NULL",
        [],
    ).map_err(|e| format!("repair titles: {}", e))? as u64;

    let orphans = conn.execute(
        r#"DELETE FROM entities
           WHERE id NOT IN (SELECT DISTINCT from_id FROM relations)
             AND id NOT IN (SELECT DISTINCT to_id FROM relations)"#,
        [],
    ).map_err(|e| format!("repair orphans: {}", e))? as u64;

    if bogus_entities + bogus_relations + bad_temporal + empty_titles + orphans > 0 {
        log::info!(
            "[mem] repair: {} bogus entities, {} bogus relations, {} impossible windows, {} empty titles, {} orphans",
            bogus_entities, bogus_relations, bad_temporal, empty_titles, orphans
        );
    }
    Ok(())
}

/// >2KB bodies go to content-addressed blob.
pub fn insert(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
) -> Result<String, String> {
    insert_with_options(conn, item, blobs_dir, false)
}

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

    let collision = exists(conn, &candidate).map_err(|e| e.to_string())?;
    let id = if collision && replace {
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
    let row: Option<(String, String, String, String, String, Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>, f64, String, String, Option<String>, i64, i64, Option<f64>, Option<f64>)> = conn
        .query_row(
            r#"SELECT id, uid, kind, scope, title, summary, area, tags_json,
                      body_inline, body_blob_path, source, importance,
                      created_at, updated_at, accessed_at, access_count, recurrence_count, graph_x, graph_y
               FROM mem_items WHERE id = ?1"#,
            params![id],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                    r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
                    r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?,
                    r.get(15)?, r.get(16)?, r.get(17)?, r.get(18)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("query mem_item: {}", e))?;

    let Some((id, uid, kind, scope, title, summary, area, tags_json,
              body_inline, body_blob_path, source, importance,
              created_at, updated_at, accessed_at, access_count, recurrence_count, graph_x, graph_y)) = row else {
        return Ok(None);
    };

    let mut body = if let Some(inline) = body_inline {
        inline
    } else if let Some(rel) = body_blob_path.as_ref() {
        read_blob(blobs_dir, rel)?
    } else {
        String::new()
    };

    if let Some(ref src) = source {
        if let Some(path) = src.strip_prefix("file:") {
            if let Ok(disk_content) = std::fs::read_to_string(path) {
                let trimmed = disk_content.trim();
                if !trimmed.is_empty() {
                    body = trimmed.to_string();
                }
            }
        }
    }

    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

    let now = chrono::Utc::now().to_rfc3339();
    let touched = conn.execute(
        "UPDATE mem_items SET accessed_at = ?1, access_count = access_count + 1 WHERE id = ?2",
        params![&now, &id],
    );
    let (new_accessed_at, new_access_count) = match touched {
        Ok(n) if n > 0 => (Some(now.clone()), access_count + 1),
        _ => (accessed_at, access_count),
    };

    Ok(Some(MemItem {
        id, uid, kind, scope, title, summary, area, tags, body, source,
        importance, created_at, updated_at,
        accessed_at: new_accessed_at,
        access_count: new_access_count,
        recurrence_count, graph_x, graph_y,
    }))
}

pub struct MemUpdate {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub area: Option<String>,
    pub tags: Option<Vec<String>>,
    pub body: Option<String>,
    pub importance: Option<f64>,
    pub bump_recurrence: Option<i64>,
}

pub fn update(
    conn: &Connection,
    id: &str,
    patch: &MemUpdate,
    blobs_dir: &Path,
) -> Result<bool, String> {
    let existing = get(conn, id, blobs_dir)?;
    let Some(prev) = existing else {
        return Ok(false);
    };

    let new_title = patch.title.clone().unwrap_or(prev.title);
    let new_summary = patch.summary.clone().or(prev.summary);
    let new_area = patch.area.clone().or(prev.area);
    let new_tags = patch.tags.clone().unwrap_or(prev.tags);
    let new_body = patch.body.clone().unwrap_or(prev.body);
    let new_importance = patch.importance.unwrap_or(prev.importance);

    let tags_json = serde_json::to_string(&new_tags)
        .map_err(|e| format!("serialize tags: {}", e))?;
    let now = chrono::Utc::now().to_rfc3339();

    let (body_inline, body_blob_path): (Option<String>, Option<String>) =
        if new_body.len() > INLINE_BODY_LIMIT {
            let path = write_blob(blobs_dir, &new_body)?;
            (None, Some(path))
        } else {
            (Some(new_body.clone()), None)
        };

    let recur_bump = patch.bump_recurrence.unwrap_or(0);
    conn.execute(
        r#"UPDATE mem_items
           SET title = ?1, summary = ?2, area = ?3, tags_json = ?4,
               body_inline = ?5, body_blob_path = ?6, importance = ?7,
               updated_at = ?8, recurrence_count = recurrence_count + ?10
           WHERE id = ?9"#,
        params![
            &new_title, &new_summary, &new_area, &tags_json,
            &body_inline, &body_blob_path, new_importance, &now, &id, recur_bump,
        ],
    ).map_err(|e| format!("update mem_item: {}", e))?;

    let tags_text = new_tags.join(" ");
    let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![&id]);
    conn.execute(
        r#"INSERT INTO mem_fts (mem_id, title, summary, body, tags)
           VALUES (?1, ?2, ?3, ?4, ?5)"#,
        params![
            &id, &new_title, new_summary.as_deref().unwrap_or(""), &new_body, &tags_text,
        ],
    ).map_err(|e| format!("update fts: {}", e))?;

    Ok(true)
}

pub fn delete(conn: &Connection, id: &str) -> Result<bool, String> {
    let n = conn
        .execute("DELETE FROM mem_items WHERE id = ?1", params![id])
        .map_err(|e| format!("delete mem_item: {}", e))?;
    let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![id]);
    let _ = conn.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        params![id, format!("{}_chunk_%", id)],
    );
    let _ = conn.execute(
        "DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1",
        params![id],
    );
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
    // mem_vectors only exists when sqlite-vec is loaded; missing = 0
    let total_vectors: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_vectors", [], |r| r.get(0))
        .unwrap_or(0);

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
        total_vectors: total_vectors as u64,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    pub importance: f64,
    pub tags: Vec<String>,
    pub has_vector: bool,
    pub has_body: bool,
    pub graph_x: Option<f64>,
    pub graph_y: Option<f64>,
    #[serde(default)]
    pub access_count: i64,
    #[serde(default)]
    pub recurrence_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub rel: String,
    pub confidence: f64,
    pub valid_from: String,
    pub valid_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDump {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub stats: GraphDumpStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDumpStats {
    pub node_count: usize,
    pub edge_count: usize,
}

pub fn graph_full(conn: &Connection) -> Result<GraphDump, String> {
    let vector_ids: std::collections::HashSet<String> = {
        let mut set = std::collections::HashSet::new();
        if let Ok(mut stmt) = conn.prepare("SELECT id FROM mem_vectors") {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for r in rows.flatten() {
                    set.insert(r);
                }
            }
        }
        set
    };

    let mut stmt = conn
        .prepare(
            r#"SELECT id, kind, title, summary, area, tags_json, importance,
                      body_inline, body_blob_path, graph_x, graph_y,
                      access_count, recurrence_count
               FROM mem_items"#,
        )
        .map_err(|e| format!("prepare graph nodes: {}", e))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<f64>>(9)?,
                r.get::<_, Option<f64>>(10)?,
                r.get::<_, i64>(11)?,
                r.get::<_, i64>(12)?,
            ))
        })
        .map_err(|e| format!("query graph nodes: {}", e))?;

    let mut nodes: Vec<GraphNode> = Vec::new();
    for row in rows {
        let (id, kind, title, summary, area, tags_json, importance,
             body_inline, body_blob_path, graph_x, graph_y, access_count, recurrence_count) =
            row.map_err(|e| format!("row graph nodes: {}", e))?;
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        let has_body = body_inline.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
            || body_blob_path.is_some();
        let has_vector = vector_ids.contains(&id);
        nodes.push(GraphNode {
            id, kind, title, summary, area, importance, tags,
            has_vector, has_body, graph_x, graph_y, access_count, recurrence_count,
        });
    }

    let node_ids: std::collections::HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();
    if let Ok(mut entity_stmt) = conn.prepare(
        "SELECT id, entity_type, properties_json FROM entities"
    ) {
        if let Ok(erows) = entity_stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        }) {
            for row in erows.flatten() {
                let (id, entity_type, props_json) = row;
                if node_ids.contains(&id) { continue; }
                let label = serde_json::from_str::<serde_json::Value>(&props_json)
                    .ok()
                    .and_then(|v| v.get("label").and_then(|l| l.as_str()).map(String::from))
                    .unwrap_or_else(|| id.clone());
                nodes.push(GraphNode {
                    id: id.clone(),
                    kind: entity_type.to_lowercase(),
                    title: label,
                    summary: None,
                    area: None,
                    importance: 3.0,
                    tags: vec!["entity".to_string()],
                    has_vector: false,
                    has_body: false,
                    graph_x: None,
                    graph_y: None,
                    access_count: 0,
                    recurrence_count: 0,
                });
            }
        }
    }

    let mut estmt = conn
        .prepare(
            r#"SELECT from_id, to_id, relation_type, confidence, valid_from, valid_to
               FROM relations
               WHERE valid_to IS NULL OR valid_to > datetime('now')"#,
        )
        .map_err(|e| format!("prepare graph edges: {}", e))?;
    let erows = estmt
        .query_map([], |r| {
            Ok(GraphEdge {
                from: r.get(0)?,
                to: r.get(1)?,
                rel: r.get(2)?,
                confidence: r.get(3)?,
                valid_from: r.get(4)?,
                valid_to: r.get(5)?,
            })
        })
        .map_err(|e| format!("query graph edges: {}", e))?;
    let all_node_ids: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.id.clone()).collect();
    let mut edges: Vec<GraphEdge> = Vec::new();
    for e in erows {
        let edge = e.map_err(|e| format!("row graph edge: {}", e))?;
        if all_node_ids.contains(&edge.from) && all_node_ids.contains(&edge.to) {
            edges.push(edge);
        }
    }

    let stats = GraphDumpStats { node_count: nodes.len(), edge_count: edges.len() };
    Ok(GraphDump { nodes, edges, stats })
}

pub fn set_positions(
    conn: &mut Connection,
    positions: &[(String, f64, f64)],
) -> Result<usize, String> {
    let tx = conn.transaction().map_err(|e| format!("begin tx: {}", e))?;
    let mut updated = 0usize;
    {
        let mut stmt = tx
            .prepare("UPDATE mem_items SET graph_x = ?1, graph_y = ?2 WHERE id = ?3")
            .map_err(|e| format!("prepare set_positions: {}", e))?;
        for (id, x, y) in positions {
            let n = stmt
                .execute(params![x, y, id])
                .map_err(|e| format!("update position: {}", e))?;
            updated += n;
        }
    }
    tx.commit().map_err(|e| format!("commit tx: {}", e))?;
    Ok(updated)
}

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
    fn get_updates_accessed_at_and_access_count() {
        let (conn, blobs) = fresh();
        let id = insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "touch me".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "body".into(),
                source: None,
                importance: 3.0,
            },
            &blobs,
        )
        .unwrap();

        // Sleep 1 ms so rfc3339 timestamps differ even on fast CI hardware.
        std::thread::sleep(std::time::Duration::from_millis(2));

        let got = get(&conn, &id, &blobs).unwrap().unwrap();
        let created = got.created_at.clone();
        let accessed = got.accessed_at.clone().expect("accessed_at should be set after get");
        assert!(
            accessed > created,
            "accessed_at ({}) must be newer than created_at ({})",
            accessed,
            created
        );
        assert_eq!(got.access_count, 1, "first get bumps access_count to 1");

        // Second get bumps again.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let got2 = get(&conn, &id, &blobs).unwrap().unwrap();
        assert_eq!(got2.access_count, 2);
        assert!(got2.accessed_at.unwrap() > accessed);
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
    fn update_partial_only_changes_provided_fields() {
        let (conn, blobs) = fresh();
        let id = insert(&conn, &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "original title".into(),
            summary: Some("original summary".into()),
            area: Some("original".into()),
            tags: vec!["a".into(), "b".into()],
            body: "original body".into(),
            source: None,
            importance: 3.0,
        }, &blobs).unwrap();

        // Update only tags + importance
        let updated = update(&conn, &id, &MemUpdate {
            title: None, summary: None, area: None,
            tags: Some(vec!["x".into(), "y".into(), "z".into()]),
            body: None,
            importance: Some(4.5),
            bump_recurrence: None,
        }, &blobs).unwrap();
        assert!(updated);

        let after = get(&conn, &id, &blobs).unwrap().unwrap();
        assert_eq!(after.title, "original title");      // unchanged
        assert_eq!(after.summary.unwrap(), "original summary"); // unchanged
        assert_eq!(after.area.unwrap(), "original");    // unchanged
        assert_eq!(after.body, "original body");        // unchanged
        assert_eq!(after.tags, vec!["x", "y", "z"]);    // changed
        assert_eq!(after.importance, 4.5);              // changed
    }

    #[test]
    fn update_returns_false_for_unknown_id() {
        let (conn, blobs) = fresh();
        let r = update(&conn, "nonexistent", &MemUpdate {
            title: Some("x".into()), summary: None, area: None,
            tags: None, body: None, importance: None, bump_recurrence: None,
        }, &blobs).unwrap();
        assert!(!r);
    }

    #[test]
    fn update_body_refreshes_fts() {
        let (conn, blobs) = fresh();
        let id = insert(&conn, &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "x".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "the quick brown fox".into(),
            source: None,
            importance: 3.0,
        }, &blobs).unwrap();
        update(&conn, &id, &MemUpdate {
            title: None, summary: None, area: None, tags: None,
            body: Some("changed to grey wolf".into()),
            importance: None, bump_recurrence: None,
        }, &blobs).unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mem_fts WHERE mem_id = ?1 AND body LIKE '%grey wolf%'",
            params![&id], |r| r.get(0)
        ).unwrap();
        assert_eq!(n, 1);
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
    fn migration_adds_graph_columns_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(mem_items)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(cols.iter().any(|c| c == "graph_x"));
        assert!(cols.iter().any(|c| c == "graph_y"));
    }

    #[test]
    fn graph_full_returns_nodes_and_edges() {
        let (mut conn, blobs) = fresh();
        let a = insert(&conn, &NewMemItem {
            kind: MemKind::Skill, scope: MemScope::Global,
            title: "alpha".into(), summary: Some("first".into()),
            area: Some("core".into()), tags: vec!["t1".into()],
            body: "body alpha".into(), source: None, importance: 4.0,
        }, &blobs).unwrap();
        let b = insert(&conn, &NewMemItem {
            kind: MemKind::Script, scope: MemScope::Global,
            title: "beta".into(), summary: None, area: None, tags: vec![],
            body: "body beta".into(), source: None, importance: 3.0,
        }, &blobs).unwrap();
        super::super::graph::insert_relation(&conn, &super::super::graph::Relation {
            from_id: a.clone(), relation_type: "has_script".into(), to_id: b.clone(),
            scope: "global".into(), valid_from: chrono::Utc::now().to_rfc3339(),
            valid_to: None, confidence: 1.0, properties: serde_json::json!({}),
        }).unwrap();

        let dump = graph_full(&conn).unwrap();
        assert_eq!(dump.stats.node_count, 2);
        assert_eq!(dump.stats.edge_count, 1);
        assert!(dump.nodes.iter().any(|n| n.id == a && n.kind == "skill" && n.has_body));
        assert!(dump.nodes.iter().any(|n| n.id == b));
        let edge = &dump.edges[0];
        assert_eq!(edge.from, a);
        assert_eq!(edge.to, b);
        assert_eq!(edge.rel, "has_script");

        // set_positions roundtrip
        let n = set_positions(&mut conn, &[(a.clone(), 12.5, -3.25)]).unwrap();
        assert_eq!(n, 1);
        let got = get(&conn, &a, &blobs).unwrap().unwrap();
        assert_eq!(got.graph_x, Some(12.5));
        assert_eq!(got.graph_y, Some(-3.25));

        // unknown id is skipped
        let n2 = set_positions(&mut conn, &[("nope".into(), 1.0, 2.0)]).unwrap();
        assert_eq!(n2, 0);
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
