use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection};

use super::{GraphDump, GraphDumpStats, GraphEdge, GraphNode, MemItem, MemStats, TailEntry};

pub fn stats(conn: &Connection) -> Result<MemStats, String> {
    let total_items: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_items", [], |r| r.get(0))
        .map_err(|e| format!("count items: {}", e))?;
    // history, not live graph; counting them inflated the relation total.
    let total_relations: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("count relations: {}", e))?;
    let total_entities: i64 = conn
        .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
        .map_err(|e| format!("count entities: {}", e))?;
    let total_vectors: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_vectors", [], |r| r.get(0))
        .unwrap_or(0);

    let mut by_kind: HashMap<String, u64> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT kind, COUNT(*) FROM mem_items WHERE kind != 'archived' GROUP BY kind")
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

/// Phase D: `{area -> {kind -> count}}`. NULL / empty areas are collapsed
/// to the literal key `"(none)"`. When `workspace` is Some, only rows with
/// `scope = "workspace:<id>"` are counted.
pub fn get_taxonomy(
    conn: &Connection,
    workspace: Option<&str>,
) -> Result<HashMap<String, HashMap<String, u64>>, String> {
    let (sql, scope_arg): (&str, Option<String>) = match workspace {
        Some(ws) => (
            r#"SELECT COALESCE(NULLIF(area, ''), '(none)') AS area_k, kind, COUNT(*)
               FROM mem_items
               WHERE scope = ?1 AND kind != 'archived'
               GROUP BY area_k, kind"#,
            Some(format!("workspace:{}", ws)),
        ),
        None => (
            r#"SELECT COALESCE(NULLIF(area, ''), '(none)') AS area_k, kind, COUNT(*)
               FROM mem_items
               WHERE kind != 'archived'
               GROUP BY area_k, kind"#,
            None,
        ),
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("prepare taxonomy: {}", e))?;
    let mut out: HashMap<String, HashMap<String, u64>> = HashMap::new();
    let mapper = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(String, String, i64)> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    };
    let collected: Vec<(String, String, i64)> = if let Some(ref s) = scope_arg {
        stmt.query_map(params![s], mapper)
            .map_err(|e| format!("query taxonomy: {}", e))?
            .filter_map(Result::ok)
            .collect()
    } else {
        stmt.query_map([], mapper)
            .map_err(|e| format!("query taxonomy: {}", e))?
            .filter_map(Result::ok)
            .collect()
    };
    for (area_k, kind, n) in collected {
        out.entry(area_k).or_default().insert(kind, n as u64);
    }
    Ok(out)
}

/// Return recent mem writes for the `mem tail` observability command.
/// - `since`: optional ISO 8601 cutoff (items with `updated_at > since`).
/// - `limit`: max rows to return (always enforced, even without `since`).
pub fn tail_recent(
    conn: &Connection,
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<TailEntry>, String> {
    // Left-join the most recent history row per item (via a correlated subquery) to surface `changed_by`. Items without history default to "user"
    // — this preserves observability for pre-Phase-C rows.
    let base_sql = r#"
        SELECT m.id, m.kind, m.title, m.area, m.updated_at,
               COALESCE(
                 (SELECT h.changed_by
                    FROM mem_item_history h
                   WHERE h.item_id = m.id
                   ORDER BY h.changed_at DESC, h.id DESC
                   LIMIT 1),
                 'user'
               ) AS changed_by
        FROM mem_items m
    "#;
    let sql = match since {
        Some(_) => format!(
            "{} WHERE m.updated_at > ?1 ORDER BY m.updated_at DESC LIMIT ?2",
            base_sql
        ),
        None => format!("{} ORDER BY m.updated_at DESC LIMIT ?1", base_sql),
    };
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("prepare tail: {}", e))?;

    let mapper = |r: &rusqlite::Row<'_>| -> rusqlite::Result<TailEntry> {
        Ok(TailEntry {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            area: r.get(3)?,
            updated_at: r.get(4)?,
            changed_by: r.get(5)?,
        })
    };
    let collected: Vec<TailEntry> = match since {
        Some(s) => stmt
            .query_map(params![s, limit as i64], mapper)
            .map_err(|e| format!("query tail: {}", e))?
            .filter_map(Result::ok)
            .collect(),
        None => stmt
            .query_map(params![limit as i64], mapper)
            .map_err(|e| format!("query tail: {}", e))?
            .filter_map(Result::ok)
            .collect(),
    };
    Ok(collected)
}

pub fn list_pinned(
    conn: &Connection,
    blobs_dir: &Path,
    limit: usize,
) -> Result<Vec<MemItem>, String> {
    let limit = limit.clamp(1, 500);
    let mut stmt = conn
        .prepare(
            "SELECT id FROM mem_items
             WHERE is_pinned_wakeup = 1 AND kind != 'archived'
             ORDER BY importance DESC, evidence_count DESC, updated_at DESC
             LIMIT ?1",
        )
        .map_err(|e| format!("prepare list_pinned: {}", e))?;
    let ids: Vec<String> = stmt
        .query_map(params![limit as i64], |r| r.get::<_, String>(0))
        .map_err(|e| format!("query list_pinned: {}", e))?
        .filter_map(Result::ok)
        .collect();

    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(item) = super::get(conn, &id, blobs_dir)? {
            out.push(item);
        }
    }
    Ok(out)
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
                      body_inline, body_blob_path,
                      access_count, recurrence_count
               FROM mem_items
               WHERE kind != 'archived'"#,
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
                r.get::<_, i64>(9)?,
                r.get::<_, i64>(10)?,
            ))
        })
        .map_err(|e| format!("query graph nodes: {}", e))?;

    let mut nodes: Vec<GraphNode> = Vec::new();
    for row in rows {
        let (
            id,
            kind,
            title,
            summary,
            area,
            tags_json,
            importance,
            body_inline,
            body_blob_path,
            access_count,
            recurrence_count,
        ) = row.map_err(|e| format!("row graph nodes: {}", e))?;
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        let has_body = body_inline
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
            || body_blob_path.is_some();
        let has_vector = vector_ids.contains(&id);
        nodes.push(GraphNode {
            id,
            kind,
            title,
            summary,
            area,
            importance,
            tags,
            has_vector,
            has_body,
            access_count,
            recurrence_count,
        });
    }

    let node_ids: std::collections::HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();
    if let Ok(mut entity_stmt) =
        conn.prepare("SELECT id, entity_type, properties_json FROM entities")
    {
        if let Ok(erows) = entity_stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }) {
            for row in erows.flatten() {
                let (id, entity_type, props_json) = row;
                if node_ids.contains(&id) {
                    continue;
                }
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
               WHERE valid_to IS NULL"#,
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

    let stats = GraphDumpStats {
        node_count: nodes.len(),
        edge_count: edges.len(),
    };
    Ok(GraphDump {
        nodes,
        edges,
        stats,
    })
}

/// Ids of dream diaries (kind `daily`, tagged `dream-diary`) created at or
/// after `since_ms`, newest first. Timestamps are compared parsed, not as text.
pub fn dream_diaries_since(conn: &Connection, since_ms: u64) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at FROM mem_items
             WHERE kind = 'daily' AND tags_json LIKE '%\"dream-diary\"%'
             ORDER BY created_at DESC, id DESC LIMIT 50",
        )
        .map_err(|e| format!("prepare dream diaries: {}", e))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| format!("query dream diaries: {}", e))?;
    let mut ids = Vec::new();
    for row in rows {
        let (id, created_at) = row.map_err(|e| format!("read dream diary: {}", e))?;
        let created_ms = chrono::DateTime::parse_from_rfc3339(&created_at)
            .map(|at| at.timestamp_millis())
            .unwrap_or(i64::MIN);
        if created_ms >= since_ms as i64 {
            ids.push(id);
        }
    }
    Ok(ids)
}
