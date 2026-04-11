//! Knowledge graph: typed entities + temporal relations.
//! Relations carry `valid_from`/`valid_to` so as-of queries can filter stale facts.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub entity_type: String,
    pub scope: String,
    pub properties: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub from_id: String,
    pub relation_type: String,
    pub to_id: String,
    pub scope: String,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub confidence: f64,
    pub properties: Value,
}

pub fn insert_entity(conn: &Connection, e: &Entity) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let props = serde_json::to_string(&e.properties).unwrap_or_else(|_| "{}".into());
    conn.execute(
        r#"INSERT OR REPLACE INTO entities
           (id, entity_type, scope, properties_json, created_at, updated_at)
           VALUES (?1, ?2, ?3, ?4,
                   COALESCE((SELECT created_at FROM entities WHERE id = ?1), ?5),
                   ?5)"#,
        params![&e.id, &e.entity_type, &e.scope, &props, &now],
    )
    .map_err(|e| format!("insert entity: {}", e))?;
    Ok(())
}

pub fn insert_relation(conn: &Connection, r: &Relation) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let props = serde_json::to_string(&r.properties).unwrap_or_else(|_| "{}".into());
    conn.execute(
        r#"INSERT OR IGNORE INTO relations
           (from_id, relation_type, to_id, scope, valid_from, valid_to,
            confidence, properties_json, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
        params![
            &r.from_id, &r.relation_type, &r.to_id, &r.scope,
            &r.valid_from, &r.valid_to, r.confidence, &props, &now,
        ],
    )
    .map_err(|e| format!("insert relation: {}", e))?;
    Ok(())
}

/// Temporal upsert: if there's already an open (valid_to IS NULL) relation
/// with the same (from, rel, to), close it at `now` and insert the new one.
/// Preserves historical trail for time-travel queries.
pub fn upsert_relation(conn: &Connection, r: &Relation) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();

    // Close any open duplicate
    conn.execute(
        r#"UPDATE relations
           SET valid_to = ?1
           WHERE from_id = ?2 AND relation_type = ?3 AND to_id = ?4
             AND valid_to IS NULL"#,
        params![&now, &r.from_id, &r.relation_type, &r.to_id],
    ).map_err(|e| format!("close open relation: {}", e))?;

    // Insert new open relation
    insert_relation(conn, r)
}

/// Invalidate an open relation — set valid_to to `ended` (or now).
/// Preserves history; does NOT delete.
pub fn invalidate_relation(
    conn: &Connection,
    from_id: &str,
    relation_type: &str,
    to_id: &str,
    ended: Option<&str>,
) -> Result<bool, String> {
    let when = ended.map(|s| s.to_string()).unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let n = conn.execute(
        r#"UPDATE relations
           SET valid_to = ?1
           WHERE from_id = ?2 AND relation_type = ?3 AND to_id = ?4
             AND valid_to IS NULL"#,
        params![&when, from_id, relation_type, to_id],
    ).map_err(|e| format!("invalidate: {}", e))?;
    Ok(n > 0)
}

/// Timeline for an entity: all relations where it's subject or object,
/// sorted by valid_from. Includes expired relations so history is visible.
pub fn timeline(
    conn: &Connection,
    entity_id: &str,
) -> Result<Vec<Relation>, String> {
    let mut stmt = conn.prepare(
        r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to,
                  confidence, properties_json
           FROM relations
           WHERE from_id = ?1 OR to_id = ?1
           ORDER BY valid_from DESC, created_at DESC"#,
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![entity_id], |r| {
        let props_json: String = r.get(7)?;
        Ok(Relation {
            from_id: r.get(0)?,
            relation_type: r.get(1)?,
            to_id: r.get(2)?,
            scope: r.get(3)?,
            valid_from: r.get(4)?,
            valid_to: r.get(5)?,
            confidence: r.get(6)?,
            properties: serde_json::from_str(&props_json).unwrap_or(Value::Null),
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Outgoing relations from `id`. Optional type filter and as-of date.
pub fn related(
    conn: &Connection,
    id: &str,
    rel_type: Option<&str>,
    as_of: Option<&str>,
) -> Result<Vec<Relation>, String> {
    let mut sql = String::from(
        r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to,
                  confidence, properties_json
           FROM relations WHERE from_id = ?1"#,
    );
    let mut binds: Vec<String> = vec![id.to_string()];
    if let Some(t) = rel_type {
        sql.push_str(" AND relation_type = ?2");
        binds.push(t.to_string());
    }
    if let Some(date) = as_of {
        let n = binds.len() + 1;
        sql.push_str(&format!(
            " AND valid_from <= ?{0} AND (valid_to IS NULL OR valid_to > ?{0})",
            n
        ));
        binds.push(date.to_string());
    }

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> =
        binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(bind_refs), |r| {
            let props_json: String = r.get(7)?;
            Ok(Relation {
                from_id: r.get(0)?,
                relation_type: r.get(1)?,
                to_id: r.get(2)?,
                scope: r.get(3)?,
                valid_from: r.get(4)?,
                valid_to: r.get(5)?,
                confidence: r.get(6)?,
                properties: serde_json::from_str(&props_json).unwrap_or(Value::Null),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct TraversalResult {
    pub nodes: Vec<String>,
    pub edges: Vec<Relation>,
    pub hops_used: usize,
}

/// BFS traversal from an entity. Respects as_of filter. Caps total nodes
/// to prevent explosion. Returns both nodes visited and edges followed.
pub fn traverse(
    conn: &Connection,
    start: &str,
    max_hops: usize,
    max_nodes: usize,
    as_of: Option<&str>,
) -> Result<TraversalResult, String> {
    use std::collections::{HashSet, VecDeque};

    let mut visited: HashSet<String> = HashSet::new();
    let mut edges: Vec<Relation> = Vec::new();
    let mut frontier: VecDeque<(String, usize)> = VecDeque::new();

    visited.insert(start.to_string());
    frontier.push_back((start.to_string(), 0));
    let mut hops_used = 0;

    while let Some((node, hop)) = frontier.pop_front() {
        if hop >= max_hops { continue; }
        if visited.len() >= max_nodes { break; }
        hops_used = hops_used.max(hop + 1);

        // Outgoing + incoming relations
        let mut sql = String::from(
            r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json
               FROM relations WHERE (from_id = ?1 OR to_id = ?1)"#,
        );
        let mut binds: Vec<String> = vec![node.clone()];
        if let Some(date) = as_of {
            sql.push_str(" AND valid_from <= ?2 AND (valid_to IS NULL OR valid_to > ?2)");
            binds.push(date.to_string());
        }

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let bind_refs: Vec<&dyn rusqlite::ToSql> =
            binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(bind_refs), |r| {
            let props_json: String = r.get(7)?;
            Ok(Relation {
                from_id: r.get(0)?,
                relation_type: r.get(1)?,
                to_id: r.get(2)?,
                scope: r.get(3)?,
                valid_from: r.get(4)?,
                valid_to: r.get(5)?,
                confidence: r.get(6)?,
                properties: serde_json::from_str(&props_json).unwrap_or(Value::Null),
            })
        }).map_err(|e| e.to_string())?;

        for row in rows {
            let rel = row.map_err(|e| e.to_string())?;
            let neighbor = if rel.from_id == node { &rel.to_id } else { &rel.from_id };
            if !visited.contains(neighbor) {
                visited.insert(neighbor.clone());
                frontier.push_back((neighbor.clone(), hop + 1));
            }
            edges.push(rel);
        }
    }

    Ok(TraversalResult {
        nodes: visited.into_iter().collect(),
        edges,
        hops_used,
    })
}

/// Shortest path between two entities using BFS. Returns sequence of relations
/// or None if unreachable within max_hops.
pub fn shortest_path(
    conn: &Connection,
    from: &str,
    to: &str,
    max_hops: usize,
) -> Result<Option<Vec<Relation>>, String> {
    use std::collections::{HashMap, VecDeque};

    if from == to { return Ok(Some(vec![])); }

    let mut visited: HashMap<String, (String, Relation)> = HashMap::new(); // node -> (prev_node, via_rel)
    let mut frontier: VecDeque<(String, usize)> = VecDeque::new();
    frontier.push_back((from.to_string(), 0));

    while let Some((node, hop)) = frontier.pop_front() {
        if hop >= max_hops { continue; }

        let mut stmt = conn.prepare(
            r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json
               FROM relations WHERE from_id = ?1 OR to_id = ?1"#,
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![&node], |r| {
            let props_json: String = r.get(7)?;
            Ok(Relation {
                from_id: r.get(0)?,
                relation_type: r.get(1)?,
                to_id: r.get(2)?,
                scope: r.get(3)?,
                valid_from: r.get(4)?,
                valid_to: r.get(5)?,
                confidence: r.get(6)?,
                properties: serde_json::from_str(&props_json).unwrap_or(Value::Null),
            })
        }).map_err(|e| e.to_string())?;

        for row in rows {
            let rel = row.map_err(|e| e.to_string())?;
            let neighbor = if rel.from_id == node { rel.to_id.clone() } else { rel.from_id.clone() };
            if visited.contains_key(&neighbor) || neighbor == from { continue; }
            visited.insert(neighbor.clone(), (node.clone(), rel));
            if neighbor == to {
                // Reconstruct path
                let mut path = Vec::new();
                let mut cur = to.to_string();
                while let Some((prev, via)) = visited.remove(&cur) {
                    path.push(via);
                    if prev == from { break; }
                    cur = prev;
                }
                path.reverse();
                return Ok(Some(path));
            }
            frontier.push_back((neighbor, hop + 1));
        }
    }

    Ok(None)
}

/// Resolve an entity id through the aliases table.
/// If the id is an alias, returns the canonical id. Otherwise returns id as-is.
pub fn resolve_canonical(conn: &Connection, id: &str) -> Result<String, String> {
    let canonical: Option<String> = conn
        .query_row(
            "SELECT canonical_id FROM entity_aliases WHERE alias_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| format!("resolve_canonical: {}", e))?;
    Ok(canonical.unwrap_or_else(|| id.to_string()))
}

/// Create an alias `from_id → to_id` (canonical).
/// Also rewires any existing relations from the alias to point at canonical.
pub fn add_alias(conn: &Connection, alias_id: &str, canonical_id: &str) -> Result<(), String> {
    if alias_id == canonical_id {
        return Err("alias cannot equal canonical".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO entity_aliases (alias_id, canonical_id, created_at) VALUES (?1, ?2, ?3)",
        params![alias_id, canonical_id, &now],
    ).map_err(|e| format!("add_alias: {}", e))?;

    // Rewire relations: any relation pointing to alias → point to canonical
    conn.execute(
        "UPDATE OR IGNORE relations SET from_id = ?1 WHERE from_id = ?2",
        params![canonical_id, alias_id],
    ).map_err(|e| format!("rewire from: {}", e))?;
    conn.execute(
        "UPDATE OR IGNORE relations SET to_id = ?1 WHERE to_id = ?2",
        params![canonical_id, alias_id],
    ).map_err(|e| format!("rewire to: {}", e))?;

    // Delete the duplicate row relations that couldn't be rewired
    conn.execute(
        "DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1",
        params![alias_id],
    ).map_err(|e| format!("cleanup alias relations: {}", e))?;

    // Remove the alias entity itself (keeping canonical)
    conn.execute(
        "DELETE FROM entities WHERE id = ?1",
        params![alias_id],
    ).map_err(|e| format!("delete alias entity: {}", e))?;

    Ok(())
}

/// List all aliases for a canonical entity.
pub fn list_aliases(conn: &Connection, canonical_id: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn.prepare(
        "SELECT alias_id FROM entity_aliases WHERE canonical_id = ?1 ORDER BY alias_id",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![canonical_id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Tunnels: entities that appear in 2+ distinct areas (cross-project bridges).
/// Returns entities sorted by area count descending.
pub fn tunnels(conn: &Connection) -> Result<Vec<serde_json::Value>, String> {
    let mut stmt = conn.prepare(
        r#"SELECT e.id, e.entity_type, COUNT(DISTINCT m.area) AS area_count,
                  GROUP_CONCAT(DISTINCT m.area) AS areas
           FROM entities e
           JOIN relations r ON (r.to_id = e.id OR r.from_id = e.id)
           JOIN mem_items m ON (m.id = r.from_id OR m.id = r.to_id)
           WHERE m.area IS NOT NULL AND m.area != ''
           GROUP BY e.id
           HAVING area_count >= 2
           ORDER BY area_count DESC
           LIMIT 50"#,
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "entity_type": r.get::<_, String>(1)?,
            "area_count": r.get::<_, i64>(2)?,
            "areas": r.get::<_, Option<String>>(3)?.unwrap_or_default().split(',').map(|s| s.to_string()).collect::<Vec<_>>(),
        }))
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::store::init_schema;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn entity_roundtrip() {
        let conn = fresh();
        let e = Entity {
            id: "person_kai".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Kai"}),
        };
        insert_entity(&conn, &e).unwrap();
        // Idempotent on second insert
        insert_entity(&conn, &e).unwrap();
    }

    #[test]
    fn relation_with_as_of_filter() {
        let conn = fresh();
        insert_relation(
            &conn,
            &Relation {
                from_id: "person_kai".into(),
                relation_type: "works_on".into(),
                to_id: "project_a".into(),
                scope: "global".into(),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: Some("2026-03-01T00:00:00Z".into()),
                confidence: 1.0,
                properties: serde_json::json!({}),
            },
        )
        .unwrap();
        insert_relation(
            &conn,
            &Relation {
                from_id: "person_kai".into(),
                relation_type: "works_on".into(),
                to_id: "project_b".into(),
                scope: "global".into(),
                valid_from: "2026-03-01T00:00:00Z".into(),
                valid_to: None,
                confidence: 1.0,
                properties: serde_json::json!({}),
            },
        )
        .unwrap();

        // As of 2026-02-01: should see project_a only
        let r = related(&conn, "person_kai", None, Some("2026-02-01T00:00:00Z")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].to_id, "project_a");

        // As of 2026-04-01: should see project_b only
        let r = related(&conn, "person_kai", None, Some("2026-04-01T00:00:00Z")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].to_id, "project_b");

        // No date filter: should see both
        let r = related(&conn, "person_kai", None, None).unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn unique_constraint_dedupes() {
        let conn = fresh();
        let r = Relation {
            from_id: "a".into(),
            relation_type: "rel".into(),
            to_id: "b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
        };
        insert_relation(&conn, &r).unwrap();
        insert_relation(&conn, &r).unwrap();
        let all = related(&conn, "a", None, None).unwrap();
        assert_eq!(all.len(), 1);
    }
}
