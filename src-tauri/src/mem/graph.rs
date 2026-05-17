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
    #[serde(default)]
    pub reason: Option<String>,
}

pub fn insert_entity(conn: &Connection, e: &Entity) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let props = serde_json::to_string(&e.properties).unwrap_or_else(|_| "{}".into());
    // Why ON CONFLICT + json_patch instead of INSERT OR REPLACE:
    //   INSERT OR REPLACE deletes the existing row entirely, so an extraction
    //   worker that re-saves person_X with `{}` (just the name + type) would
    //   wipe email/phone/role that were set by an earlier richer save.
    //   json_patch(existing, incoming) field-merges with incoming wins on
    //   collision. entity_type / scope are KEPT (the first creator knew the
    //   right type; later stubs shouldn't override).
    conn.execute(
        r#"INSERT INTO entities
           (id, entity_type, scope, properties_json, created_at, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?5)
           ON CONFLICT(id) DO UPDATE SET
             properties_json = json_patch(properties_json, excluded.properties_json),
             updated_at = excluded.updated_at"#,
        params![&e.id, &e.entity_type, &e.scope, &props, &now],
    )
    .map_err(|e| format!("insert entity: {}", e))?;

    // Embed the entity (id + type + properties) into mem_vectors so unified
    // `mem search` can find it semantically. Best-effort — embedding failures
    // log a warning but don't fail the upsert.
    let merged_props: String = conn
        .query_row(
            "SELECT properties_json FROM entities WHERE id = ?1",
            params![&e.id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| props.clone());
    let text = super::embeddings::embeddable_text_for_entity(
        &e.id,
        &e.entity_type,
        &merged_props,
    );
    if let Err(err) = super::embeddings::index(conn, &e.id, &text) {
        log::warn!("[graph.insert_entity] embed {} failed: {}", e.id, err);
    }
    Ok(())
}

pub fn insert_relation(conn: &Connection, r: &Relation) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let props = serde_json::to_string(&r.properties).unwrap_or_else(|_| "{}".into());
    conn.execute(
        r#"INSERT OR IGNORE INTO relations
           (from_id, relation_type, to_id, scope, valid_from, valid_to,
            confidence, properties_json, created_at, reason)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
        params![
            &r.from_id, &r.relation_type, &r.to_id, &r.scope,
            &r.valid_from, &r.valid_to, r.confidence, &props, &now, &r.reason,
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

/// Timeline for an entity: relations where it's subject or object,
/// sorted by valid_from. Default: active-only. Pass `include_invalidated = true`
/// for full history (including closed `valid_to` rows).
pub fn timeline(
    conn: &Connection,
    entity_id: &str,
    include_invalidated: bool,
) -> Result<Vec<Relation>, String> {
    let mut sql = String::from(
        r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to,
                  confidence, properties_json
           FROM relations
           WHERE (from_id = ?1 OR to_id = ?1)"#,
    );
    let mut binds: Vec<String> = vec![entity_id.to_string()];
    append_active_filter(&mut sql, &mut binds, include_invalidated, None);
    sql.push_str(" ORDER BY valid_from DESC, created_at DESC");

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
            reason: None,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Append the active-window filter to a relations SELECT.
/// - `include_invalidated`: pass-through, no filter.
/// - `as_of` set: time-travel — `valid_from <= as_of AND (valid_to IS NULL OR datetime(valid_to) > datetime(as_of))`.
/// - default: `valid_to IS NULL` (active-only).
fn append_active_filter(
    sql: &mut String,
    binds: &mut Vec<String>,
    include_invalidated: bool,
    as_of: Option<&str>,
) {
    if include_invalidated {
        return;
    }
    if let Some(date) = as_of {
        let n = binds.len() + 1;
        sql.push_str(&format!(
            " AND valid_from <= ?{0} AND (valid_to IS NULL OR datetime(valid_to) > datetime(?{0}))",
            n
        ));
        binds.push(date.to_string());
    } else {
        sql.push_str(" AND valid_to IS NULL");
    }
}

/// Relations connected to `id` (both incoming and outgoing).
/// Default: active-only. Pass `include_invalidated = true` for full history,
/// or `as_of = Some(date)` for time-travel reads.
pub fn related(
    conn: &Connection,
    id: &str,
    rel_type: Option<&str>,
    as_of: Option<&str>,
    include_invalidated: bool,
) -> Result<Vec<Relation>, String> {
    let mut sql = String::from(
        r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to,
                  confidence, properties_json
           FROM relations WHERE (from_id = ?1 OR to_id = ?1)"#,
    );
    let mut binds: Vec<String> = vec![id.to_string()];
    if let Some(t) = rel_type {
        sql.push_str(" AND relation_type = ?2");
        binds.push(t.to_string());
    }
    append_active_filter(&mut sql, &mut binds, include_invalidated, as_of);

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
                reason: None,
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

/// BFS traversal from an entity. Default: active-only. Respects `as_of` for
/// time-travel reads, or skip the filter via `include_invalidated`.
pub fn traverse(
    conn: &Connection,
    start: &str,
    max_hops: usize,
    max_nodes: usize,
    as_of: Option<&str>,
    include_invalidated: bool,
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

        let mut sql = String::from(
            r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json
               FROM relations WHERE (from_id = ?1 OR to_id = ?1)"#,
        );
        let mut binds: Vec<String> = vec![node.clone()];
        append_active_filter(&mut sql, &mut binds, include_invalidated, as_of);

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
                reason: None,
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

/// Shortest path between two entities using BFS. Default: active-only.
/// Pass `include_invalidated = true` to walk historical edges too.
pub fn shortest_path(
    conn: &Connection,
    from: &str,
    to: &str,
    max_hops: usize,
    include_invalidated: bool,
) -> Result<Option<Vec<Relation>>, String> {
    use std::collections::{HashMap, VecDeque};

    if from == to { return Ok(Some(vec![])); }

    let mut visited: HashMap<String, (String, Relation)> = HashMap::new(); // node -> (prev_node, via_rel)
    let mut frontier: VecDeque<(String, usize)> = VecDeque::new();
    frontier.push_back((from.to_string(), 0));

    while let Some((node, hop)) = frontier.pop_front() {
        if hop >= max_hops { continue; }

        let mut sql = String::from(
            r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json
               FROM relations WHERE (from_id = ?1 OR to_id = ?1)"#,
        );
        let mut binds: Vec<String> = vec![node.clone()];
        append_active_filter(&mut sql, &mut binds, include_invalidated, None);
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
                reason: None,
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
    let tx = conn.unchecked_transaction().map_err(|e| format!("add_alias transaction: {}", e))?;
    tx.execute(
        "INSERT OR REPLACE INTO entity_aliases (alias_id, canonical_id, created_at) VALUES (?1, ?2, ?3)",
        params![alias_id, canonical_id, &now],
    ).map_err(|e| format!("add_alias: {}", e))?;

    // Rewire relations: any relation pointing to alias → point to canonical
    tx.execute(
        "UPDATE OR IGNORE relations SET from_id = ?1 WHERE from_id = ?2",
        params![canonical_id, alias_id],
    ).map_err(|e| format!("rewire from: {}", e))?;
    tx.execute(
        "UPDATE OR IGNORE relations SET to_id = ?1 WHERE to_id = ?2",
        params![canonical_id, alias_id],
    ).map_err(|e| format!("rewire to: {}", e))?;

    // Delete the duplicate row relations that couldn't be rewired
    tx.execute(
        "DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1",
        params![alias_id],
    ).map_err(|e| format!("cleanup alias relations: {}", e))?;

    // Carryover: fold alias props into canonical (canonical wins on collision).
    let alias_props: Option<String> = tx.query_row(
        "SELECT properties_json FROM entities WHERE id = ?1",
        params![alias_id], |r| r.get(0),
    ).ok();
    let canonical_props: Option<String> = tx.query_row(
        "SELECT properties_json FROM entities WHERE id = ?1",
        params![canonical_id], |r| r.get(0),
    ).ok();
    if let (Some(a), Some(c)) = (alias_props, canonical_props) {
        let alias_obj = serde_json::from_str::<Value>(&a).ok()
            .and_then(|v| v.as_object().cloned()).unwrap_or_default();
        let mut canon_obj = serde_json::from_str::<Value>(&c).ok()
            .and_then(|v| v.as_object().cloned()).unwrap_or_default();
        for (k, v) in alias_obj {
            canon_obj.entry(k).or_insert(v);
        }
        let merged = serde_json::to_string(&Value::Object(canon_obj))
            .map_err(|e| format!("merge props: {}", e))?;
        tx.execute(
            "UPDATE entities SET properties_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![merged, &now, canonical_id],
        ).map_err(|e| format!("write merged props: {}", e))?;
    }

    // Remove the alias entity itself (keeping canonical)
    tx.execute(
        "DELETE FROM entities WHERE id = ?1",
        params![alias_id],
    ).map_err(|e| format!("delete alias entity: {}", e))?;

    let _ = tx.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        params![alias_id, format!("{}_chunk_%", alias_id)],
    );

    tx.commit().map_err(|e| format!("add_alias commit: {}", e))?;
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
/// Returns entities sorted by area count descending. Default: active relations
/// only. Pass `include_invalidated = true` to walk historical edges.
pub fn tunnels(conn: &Connection, include_invalidated: bool) -> Result<Vec<serde_json::Value>, String> {
    let active_clause = if include_invalidated { "" } else { " AND r.valid_to IS NULL" };
    let sql = format!(
        r#"SELECT e.id, e.entity_type, COUNT(DISTINCT m.area) AS area_count,
                  GROUP_CONCAT(DISTINCT m.area) AS areas
           FROM entities e
           JOIN relations r ON (r.to_id = e.id OR r.from_id = e.id)
           JOIN mem_items m ON (m.id = r.from_id OR m.id = r.to_id)
           WHERE m.area IS NOT NULL AND m.area != ''
             AND e.id NOT LIKE 'kind\_%' ESCAPE '\'
             AND e.id NOT LIKE 'area\_%' ESCAPE '\'{}
           GROUP BY e.id
           HAVING area_count >= 2
           ORDER BY area_count DESC
           LIMIT 50"#,
        active_clause,
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
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
                reason: None,
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
                reason: None,
            },
        )
        .unwrap();

        // As of 2026-02-01: should see project_a only
        let r = related(&conn, "person_kai", None, Some("2026-02-01T00:00:00Z"), false).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].to_id, "project_a");

        // As of 2026-04-01: should see project_b only
        let r = related(&conn, "person_kai", None, Some("2026-04-01T00:00:00Z"), false).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].to_id, "project_b");

        // Default (active-only): only the open relation (project_b).
        let r = related(&conn, "person_kai", None, None, false).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].to_id, "project_b");

        // include_invalidated=true returns full history.
        let r = related(&conn, "person_kai", None, None, true).unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn related_is_bidirectional() {
        let conn = fresh();
        insert_relation(
            &conn,
            &Relation {
                from_id: "note_a".into(),
                relation_type: "mentions".into(),
                to_id: "person_alex".into(),
                scope: "global".into(),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                confidence: 1.0,
                properties: serde_json::json!({}),
                reason: None,
            },
        )
        .unwrap();

        // Query target entity — should find the incoming relation
        let r = related(&conn, "person_alex", None, None, false).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].from_id, "note_a");
        assert_eq!(r[0].to_id, "person_alex");

        // Query source — should also find it
        let r = related(&conn, "note_a", None, None, false).unwrap();
        assert_eq!(r.len(), 1);
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
            reason: None,
        };
        insert_relation(&conn, &r).unwrap();
        insert_relation(&conn, &r).unwrap();
        let all = related(&conn, "a", None, None, false).unwrap();
        assert_eq!(all.len(), 1);
    }

    fn pair_entities(conn: &Connection) {
        insert_entity(conn, &Entity {
            id: "e_a".into(), entity_type: "Person".into(),
            scope: "global".into(), properties: serde_json::Value::Null,
        }).unwrap();
        insert_entity(conn, &Entity {
            id: "e_b".into(), entity_type: "Person".into(),
            scope: "global".into(), properties: serde_json::Value::Null,
        }).unwrap();
    }

    fn active_rel() -> Relation {
        Relation {
            from_id: "e_a".into(), relation_type: "knows".into(), to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00.000000+00:00".into(),
            valid_to: None,
            confidence: 1.0, properties: serde_json::Value::Null,
            reason: None,
        }
    }

    fn dead_rel() -> Relation {
        Relation {
            from_id: "e_a".into(), relation_type: "loved".into(), to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2024-01-01T00:00:00.000000+00:00".into(),
            valid_to: Some("2025-06-01T00:00:00.000000+00:00".into()),
            confidence: 1.0, properties: serde_json::Value::Null,
            reason: None,
        }
    }

    #[test]
    fn related_excludes_invalidated_by_default() {
        let conn = fresh();
        pair_entities(&conn);
        insert_relation(&conn, &active_rel()).unwrap();
        insert_relation(&conn, &dead_rel()).unwrap();

        let rs = related(&conn, "e_a", None, None, false).unwrap();
        assert_eq!(rs.len(), 1, "exactly one active relation");
        assert_eq!(rs[0].valid_to, None);
    }

    #[test]
    fn related_include_invalidated_returns_both() {
        let conn = fresh();
        pair_entities(&conn);
        insert_relation(&conn, &active_rel()).unwrap();
        insert_relation(&conn, &dead_rel()).unwrap();

        let rs = related(&conn, "e_a", None, None, true).unwrap();
        assert_eq!(rs.len(), 2);
    }

    #[test]
    fn traverse_excludes_invalidated_by_default() {
        let conn = fresh();
        pair_entities(&conn);
        insert_relation(&conn, &active_rel()).unwrap();
        insert_relation(&conn, &dead_rel()).unwrap();

        let res = traverse(&conn, "e_a", 2, 100, None, false).unwrap();
        let kinds: std::collections::HashSet<&str> =
            res.edges.iter().map(|r| r.relation_type.as_str()).collect();
        assert_eq!(kinds, std::collections::HashSet::from(["knows"]),
            "only the active edge is followed; dead_rel must not appear");
    }

    #[test]
    fn shortest_path_skips_invalidated_edges() {
        let conn = fresh();
        // A -> B (dead) and A -> C -> B (alive): path must go through C
        for id in ["e_a", "e_b", "e_c"] {
            insert_entity(&conn, &Entity {
                id: id.into(), entity_type: "X".into(),
                scope: "global".into(), properties: serde_json::Value::Null,
            }).unwrap();
        }
        insert_relation(&conn, &Relation {
            from_id: "e_a".into(), relation_type: "direct".into(), to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2024-01-01T00:00:00.000000+00:00".into(),
            valid_to: Some("2025-01-01T00:00:00.000000+00:00".into()),
            confidence: 1.0, properties: serde_json::Value::Null,
            reason: None,
        }).unwrap();
        insert_relation(&conn, &Relation {
            from_id: "e_a".into(), relation_type: "via".into(), to_id: "e_c".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00.000000+00:00".into(),
            valid_to: None,
            confidence: 1.0, properties: serde_json::Value::Null,
            reason: None,
        }).unwrap();
        insert_relation(&conn, &Relation {
            from_id: "e_c".into(), relation_type: "via".into(), to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00.000000+00:00".into(),
            valid_to: None,
            confidence: 1.0, properties: serde_json::Value::Null,
            reason: None,
        }).unwrap();

        let path = shortest_path(&conn, "e_a", "e_b", 4, false).unwrap().unwrap();
        assert_eq!(path.len(), 2, "two-hop path through e_c");
        assert!(path.iter().all(|r| r.relation_type == "via"));
    }

    #[test]
    fn tunnels_excludes_entities_only_on_invalidated_edges() {
        let conn = fresh();
        let now = chrono::Utc::now().to_rfc3339();
        // Two mem_items with distinct areas + an entity
        for (id, area) in [("m_a", "alpha"), ("m_b", "beta")] {
            conn.execute(
                "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, area, created_at, updated_at)
                 VALUES (?1, ?1, 'note', 'global', ?1, '[]', ?2, ?3, ?3)",
                params![id, area, &now],
            ).unwrap();
        }
        insert_entity(&conn, &Entity {
            id: "person_only_dead".into(), entity_type: "Person".into(),
            scope: "global".into(), properties: serde_json::Value::Null,
        }).unwrap();
        // ONLY invalidated edges connect this entity to the two areas
        for src in ["m_a", "m_b"] {
            insert_relation(&conn, &Relation {
                from_id: src.into(), relation_type: "mentions".into(),
                to_id: "person_only_dead".into(), scope: "global".into(),
                valid_from: "2024-01-01T00:00:00.000000+00:00".into(),
                valid_to: Some("2025-06-01T00:00:00.000000+00:00".into()),
                confidence: 1.0, properties: serde_json::Value::Null,
                reason: None,
            }).unwrap();
        }

        let tnls = tunnels(&conn, false).unwrap();
        let ids: Vec<String> = tnls.iter()
            .filter_map(|v| v.get("id").and_then(|s| s.as_str()).map(String::from))
            .collect();
        assert!(!ids.contains(&"person_only_dead".to_string()),
            "entity reachable only via invalidated relations must NOT appear in tunnels");
    }

    #[test]
    fn timeline_excludes_invalidated_by_default() {
        let conn = fresh();
        insert_entity(&conn, &Entity {
            id: "e_a".into(), entity_type: "X".into(),
            scope: "global".into(), properties: serde_json::Value::Null,
        }).unwrap();
        insert_entity(&conn, &Entity {
            id: "e_b".into(), entity_type: "X".into(),
            scope: "global".into(), properties: serde_json::Value::Null,
        }).unwrap();
        insert_relation(&conn, &Relation {
            from_id: "e_a".into(), relation_type: "active".into(), to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00.000000+00:00".into(),
            valid_to: None,
            confidence: 1.0, properties: serde_json::Value::Null,
            reason: None,
        }).unwrap();
        insert_relation(&conn, &Relation {
            from_id: "e_a".into(), relation_type: "dead".into(), to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2025-01-01T00:00:00.000000+00:00".into(),
            valid_to: Some("2025-06-01T00:00:00.000000+00:00".into()),
            confidence: 1.0, properties: serde_json::Value::Null,
            reason: None,
        }).unwrap();

        let active = timeline(&conn, "e_a", false).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].relation_type, "active");

        let all = timeline(&conn, "e_a", true).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn add_alias_rewires_relations() {
        let conn = fresh();
        let now = "2026-01-01T00:00:00Z";
        insert_entity(&conn, &Entity { id: "alias".into(), entity_type: "Person".into(), scope: "global".into(), properties: serde_json::Value::Null }).unwrap();
        insert_entity(&conn, &Entity { id: "canonical".into(), entity_type: "Person".into(), scope: "global".into(), properties: serde_json::Value::Null }).unwrap();
        conn.execute("INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at) VALUES ('alias', 'knows', 'other', 'global', ?1, NULL, 1.0, '{}', ?1)", rusqlite::params![now]).unwrap();

        add_alias(&conn, "alias", "canonical").unwrap();

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM entities WHERE id = 'alias'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0, "alias entity should be deleted");

        let from: String = conn.query_row("SELECT from_id FROM relations WHERE relation_type = 'knows'", [], |r| r.get(0)).unwrap();
        assert_eq!(from, "canonical", "relation should be rewired to canonical");
    }

    #[test]
    fn add_alias_carries_over_properties_into_canonical() {
        let conn = fresh();
        insert_entity(&conn, &Entity {
            id: "canonical".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Andrey", "shared": "canonical_wins"}),
        }).unwrap();
        insert_entity(&conn, &Entity {
            id: "alias".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"alt_name": "", "shared": "alias_loses"}),
        }).unwrap();

        add_alias(&conn, "alias", "canonical").unwrap();

        let props: String = conn.query_row(
            "SELECT properties_json FROM entities WHERE id = 'canonical'",
            [], |r| r.get(0)
        ).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&props).unwrap();
        assert_eq!(parsed["name"], "Andrey");
        assert_eq!(parsed["alt_name"], "", "non-colliding alias props must carry over");
        assert_eq!(parsed["shared"], "canonical_wins", "canonical wins on collision");
    }

    #[test]
    fn add_alias_cleans_alias_vectors() {
        let conn = fresh();
        conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)").unwrap();
        insert_entity(&conn, &Entity {
            id: "alias".into(), entity_type: "Person".into(),
            scope: "global".into(), properties: serde_json::Value::Null,
        }).unwrap();
        insert_entity(&conn, &Entity {
            id: "canonical".into(), entity_type: "Person".into(),
            scope: "global".into(), properties: serde_json::Value::Null,
        }).unwrap();
        conn.execute(
            "INSERT INTO mem_vectors VALUES ('alias', X''), ('alias_chunk_1', X''), ('canonical', X'')",
            [],
        ).unwrap();

        add_alias(&conn, "alias", "canonical").unwrap();

        let alias_vecs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mem_vectors WHERE mem_id = 'alias' OR mem_id LIKE 'alias_chunk_%'",
            [], |r| r.get(0)
        ).unwrap();
        assert_eq!(alias_vecs, 0, "alias vectors (incl. chunks) must be deleted");
        let canonical_vecs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mem_vectors WHERE mem_id = 'canonical'",
            [], |r| r.get(0)
        ).unwrap();
        assert_eq!(canonical_vecs, 1, "canonical vectors must stay");
    }

    #[test]
    fn insert_relation_persists_reason() {
        let conn = fresh();
        pair_entities(&conn);
        let r = Relation {
            from_id: "e_a".into(),
            relation_type: "uses".into(),
            to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
            reason: Some("dreamer-bridge".into()),
        };
        insert_relation(&conn, &r).unwrap();

        let got: Option<String> = conn.query_row(
            "SELECT reason FROM relations WHERE from_id='e_a' AND to_id='e_b' AND relation_type='uses'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(got.as_deref(), Some("dreamer-bridge"));
    }

    #[test]
    fn insert_relation_null_reason() {
        let conn = fresh();
        pair_entities(&conn);
        let r = Relation {
            from_id: "e_a".into(),
            relation_type: "uses".into(),
            to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
            reason: None,
        };
        insert_relation(&conn, &r).unwrap();
        let got: Option<String> = conn.query_row(
            "SELECT reason FROM relations WHERE from_id='e_a'",
            [], |r| r.get(0),
        ).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn iso_t_separator_does_not_leak_through_string_compare() {
        let conn = fresh();
        pair_entities(&conn);
        let dead = Relation {
            from_id: "e_a".into(), relation_type: "k".into(), to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2025-01-01T00:00:00.000000+00:00".into(),
            valid_to: Some("2025-06-01T00:00:00.000000+00:00".into()),
            confidence: 1.0, properties: serde_json::Value::Null,
            reason: None,
        };
        insert_relation(&conn, &dead).unwrap();
        let rs = related(&conn, "e_a", None, None, false).unwrap();
        assert!(rs.is_empty(), "ISO-T-separated invalidated row must NOT leak");
    }
}
