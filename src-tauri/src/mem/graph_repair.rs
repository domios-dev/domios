//! Repair primitives for the graph: find/delete/merge duplicates,
//! augment entity properties. All destructive ops audit-log to
//! `graph_purge_log`. No raw SQL passes through this surface —
//! callers (Dreamer, CLI, REST) pick from typed enums only.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::graph::Relation;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationColumn {
    FromId,
    RelationType,
    ToId,
    Scope,
    ValidFrom,
}

impl RelationColumn {
    fn sql(self) -> &'static str {
        match self {
            Self::FromId => "from_id",
            Self::RelationType => "relation_type",
            Self::ToId => "to_id",
            Self::Scope => "scope",
            Self::ValidFrom => "valid_from",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateRelationGroup {
    pub key: Value,
    pub rowids: Vec<i64>,
    pub count: usize,
    pub sample: Relation,
}

pub fn find_duplicate_relations(
    conn: &Connection,
    group_by: &[RelationColumn],
    limit: usize,
) -> Result<Vec<DuplicateRelationGroup>, String> {
    if group_by.is_empty() {
        return Err("group_by must contain at least one column".into());
    }
    let cols = group_by.iter().map(|c| c.sql()).collect::<Vec<_>>().join(",");

    let sql = format!(
        "SELECT {cols}, GROUP_CONCAT(id) AS rowids, COUNT(*) AS c
         FROM relations
         WHERE valid_to IS NULL
         GROUP BY {cols}
         HAVING c > 1
         ORDER BY c DESC
         LIMIT ?1",
        cols = cols
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params![limit as i64]).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut key = Map::<String, Value>::new();
        for (i, col) in group_by.iter().enumerate() {
            let v: Option<String> = row.get(i).map_err(|e| e.to_string())?;
            key.insert(col.sql().into(), v.map(Value::String).unwrap_or(Value::Null));
        }
        let rowids_csv: String = row.get(group_by.len()).map_err(|e| e.to_string())?;
        let count: i64 = row.get(group_by.len() + 1).map_err(|e| e.to_string())?;
        let rowids: Vec<i64> = rowids_csv.split(',')
            .map(|s| s.parse::<i64>().map_err(|e| format!("parse rowid '{}': {}", s, e)))
            .collect::<Result<_, _>>()?;
        let first_rowid = *rowids.first().ok_or("empty rowid group from GROUP_CONCAT")?;
        let sample = load_relation_by_rowid(conn, first_rowid)?;
        out.push(DuplicateRelationGroup {
            key: Value::Object(key),
            rowids,
            count: count as usize,
            sample,
        });
    }
    Ok(out)
}

pub fn delete_relations_by_rowids(
    conn: &Connection,
    rowids: &[i64],
    actor: &str,
) -> Result<u64, String> {
    if rowids.is_empty() {
        return Ok(0);
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let placeholders = std::iter::repeat("?").take(rowids.len()).collect::<Vec<_>>().join(",");
    let payload_sql = format!(
        "SELECT id, from_id, relation_type, to_id, scope, valid_from, valid_to,
                confidence, properties_json, created_at
         FROM relations WHERE id IN ({})",
        placeholders
    );
    let mut stmt = tx.prepare(&payload_sql).map_err(|e| e.to_string())?;
    let bind: Vec<&dyn rusqlite::ToSql> = rowids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let payload: Vec<Value> = stmt.query_map(rusqlite::params_from_iter(bind.iter()), |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?,
            "from_id": r.get::<_, String>(1)?,
            "relation_type": r.get::<_, String>(2)?,
            "to_id": r.get::<_, String>(3)?,
            "scope": r.get::<_, String>(4)?,
            "valid_from": r.get::<_, String>(5)?,
            "valid_to": r.get::<_, Option<String>>(6)?,
            "confidence": r.get::<_, f64>(7)?,
            "properties_json": r.get::<_, String>(8)?,
            "created_at": r.get::<_, String>(9)?,
        }))
    }).map_err(|e| e.to_string())?
      .collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    drop(stmt);

    let del_sql = format!("DELETE FROM relations WHERE id IN ({})", placeholders);
    let bind: Vec<&dyn rusqlite::ToSql> = rowids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let deleted = tx.execute(&del_sql, rusqlite::params_from_iter(bind.iter()))
        .map_err(|e| e.to_string())? as u64;

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({"op": "delete_relations_by_rowids", "payload": payload}).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, deleted as i64, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(deleted)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeReport {
    pub merged_into: i64,
    pub deleted_rowids: Vec<i64>,
    pub final_confidence: f64,
}

pub fn merge_relations(
    conn: &Connection,
    keep: i64,
    merge: &[i64],
    actor: &str,
) -> Result<MergeReport, String> {
    if merge.is_empty() {
        return Err("merge list cannot be empty".into());
    }
    if merge.contains(&keep) {
        return Err("keep rowid cannot appear in merge list".into());
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let mut all_ids = vec![keep];
    all_ids.extend_from_slice(merge);
    let placeholders = std::iter::repeat("?").take(all_ids.len()).collect::<Vec<_>>().join(",");
    let bind: Vec<&dyn rusqlite::ToSql> = all_ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();

    let mut stmt = tx.prepare(&format!(
        "SELECT id, confidence, properties_json FROM relations WHERE id IN ({})", placeholders
    )).map_err(|e| e.to_string())?;
    let rows: Vec<(i64, f64, String)> = stmt.query_map(rusqlite::params_from_iter(bind.iter()), |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    }).map_err(|e| e.to_string())?
      .collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    drop(stmt);

    if rows.len() != all_ids.len() {
        return Err(format!("expected {} rows, got {} — some rowids do not exist", all_ids.len(), rows.len()));
    }

    let keep_row = rows.iter().find(|(id, _, _)| *id == keep)
        .ok_or_else(|| format!("keep row {} not found", keep))?;
    let mut keep_props: Map<String, Value> = serde_json::from_str::<Value>(&keep_row.2)
        .map_err(|e| format!("parse keep props (rowid {}): {}", keep, e))?
        .as_object().cloned().unwrap_or_default();

    let mut max_conf = keep_row.1;
    for (id, conf, props_json) in &rows {
        if *id == keep { continue; }
        if *conf > max_conf { max_conf = *conf; }
        let other: Value = serde_json::from_str(props_json)
            .map_err(|e| format!("parse merge props (rowid {}): {}", id, e))?;
        if let Some(obj) = other.as_object() {
            for (k, v) in obj {
                keep_props.entry(k.clone()).or_insert(v.clone());
            }
        }
    }

    let merged_props_json = serde_json::to_string(&Value::Object(keep_props.clone()))
        .map_err(|e| e.to_string())?;

    tx.execute(
        "UPDATE relations SET confidence = ?1, properties_json = ?2 WHERE id = ?3",
        params![max_conf, merged_props_json, keep],
    ).map_err(|e| e.to_string())?;

    let merge_placeholders = std::iter::repeat("?").take(merge.len()).collect::<Vec<_>>().join(",");
    let merge_bind: Vec<&dyn rusqlite::ToSql> = merge.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let deleted = tx.execute(
        &format!("DELETE FROM relations WHERE id IN ({})", merge_placeholders),
        rusqlite::params_from_iter(merge_bind.iter()),
    ).map_err(|e| e.to_string())? as i64;

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "op": "merge_relations",
        "kept": keep,
        "merged_in": merge,
        "final_confidence": max_conf,
        "final_properties": Value::Object(keep_props),
    }).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, deleted, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(MergeReport {
        merged_into: keep,
        deleted_rowids: merge.to_vec(),
        final_confidence: max_conf,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntitySimilarityStrategy {
    /// Same entity_type + same `properties_json.name` (case-insensitive trim).
    ExactName,
    /// Entities that resolve to the same canonical via existing aliases.
    AliasChain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateEntityGroup {
    pub canonical_hint: String,
    pub member_ids: Vec<String>,
    pub entity_type: String,
    pub reason: String,
}

pub fn find_duplicate_entities(
    conn: &Connection,
    strategy: EntitySimilarityStrategy,
    limit: usize,
) -> Result<Vec<DuplicateEntityGroup>, String> {
    match strategy {
        EntitySimilarityStrategy::ExactName => {
            let sql = "
                SELECT entity_type,
                       LOWER(TRIM(json_extract(properties_json, '$.name'))) AS norm_name,
                       GROUP_CONCAT(id) AS ids,
                       COUNT(*) AS c
                FROM entities
                WHERE json_extract(properties_json, '$.name') IS NOT NULL
                GROUP BY entity_type, norm_name
                HAVING c > 1
                ORDER BY c DESC
                LIMIT ?1";
            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![limit as i64], |r| {
                let entity_type: String = r.get(0)?;
                let norm_name: String = r.get(1)?;
                let ids_csv: String = r.get(2)?;
                Ok(DuplicateEntityGroup {
                    canonical_hint: norm_name,
                    member_ids: ids_csv.split(',').map(String::from).collect(),
                    entity_type,
                    reason: "exact_name".into(),
                })
            }).map_err(|e| e.to_string())?;
            rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
        }
        EntitySimilarityStrategy::AliasChain => {
            let sql = "
                SELECT canonical_id, GROUP_CONCAT(alias_id) AS aliases, COUNT(*) AS c
                FROM entity_aliases
                GROUP BY canonical_id
                ORDER BY c DESC
                LIMIT ?1";
            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![limit as i64], |r| {
                let canonical: String = r.get(0)?;
                let aliases_csv: String = r.get(1)?;
                let mut members: Vec<String> = aliases_csv.split(',').map(String::from).collect();
                members.push(canonical.clone());
                Ok(DuplicateEntityGroup {
                    canonical_hint: canonical,
                    member_ids: members,
                    entity_type: "Unknown".into(),
                    reason: "alias_chain".into(),
                })
            }).map_err(|e| e.to_string())?;
            rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
        }
    }
}

pub fn augment_entity(
    conn: &Connection,
    entity_id: &str,
    patch: Map<String, Value>,
    actor: &str,
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let (entity_type, existing): (String, String) = tx.query_row(
        "SELECT entity_type, properties_json FROM entities WHERE id = ?1",
        params![entity_id], |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|e| format!("entity {} not found: {}", entity_id, e))?;

    let mut props: Map<String, Value> = serde_json::from_str::<Value>(&existing)
        .map_err(|e| format!("parse existing props for {}: {}", entity_id, e))?
        .as_object().cloned().unwrap_or_default();
    for (k, v) in &patch {
        props.insert(k.clone(), v.clone());
    }
    let merged = serde_json::to_string(&Value::Object(props.clone()))
        .map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE entities SET properties_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![merged, &now, entity_id],
    ).map_err(|e| e.to_string())?;

    let text = super::embeddings::embeddable_text_for_entity(entity_id, &entity_type, &merged);
    if let Err(err) = super::embeddings::index(&tx, entity_id, &text) {
        log::warn!("[augment_entity] re-embed {} failed: {}", entity_id, err);
    }

    let details = serde_json::json!({
        "op": "augment_entity",
        "entity_id": entity_id,
        "patch": Value::Object(patch),
        "result": Value::Object(props),
    }).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, 0, NULL, NULL, NULL, ?2, ?3)",
        params![now, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteEntityReport {
    pub entity_id: String,
    pub cascaded_relations: u64,
    pub cascaded_history: u64,
}

pub fn delete_entity_by_id(
    conn: &Connection,
    entity_id: &str,
    force: bool,
    purge_history: bool,
    actor: &str,
) -> Result<DeleteEntityReport, String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let (entity_type, scope, properties_json): (String, String, String) = tx.query_row(
        "SELECT entity_type, scope, properties_json FROM entities WHERE id = ?1",
        params![entity_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).map_err(|e| format!("entity {} not found: {}", entity_id, e))?;

    let active_rels: i64 = tx.query_row(
        "SELECT COUNT(*) FROM relations
         WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)",
        params![entity_id], |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    if active_rels > 0 && !force {
        return Err(format!(
            "entity {} has {} active relations; pass force=true to cascade-delete them",
            entity_id, active_rels
        ));
    }

    let cascaded_rels: Vec<Value> = if active_rels > 0 {
        let mut stmt = tx.prepare(
            "SELECT id, from_id, relation_type, to_id, scope, valid_from, valid_to,
                    confidence, properties_json, created_at, reason
             FROM relations WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)"
        ).map_err(|e| e.to_string())?;
        let rows: Vec<Value> = stmt.query_map(params![entity_id], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "from_id": r.get::<_, String>(1)?,
                "relation_type": r.get::<_, String>(2)?,
                "to_id": r.get::<_, String>(3)?,
                "scope": r.get::<_, String>(4)?,
                "valid_from": r.get::<_, String>(5)?,
                "valid_to": r.get::<_, Option<String>>(6)?,
                "confidence": r.get::<_, f64>(7)?,
                "properties_json": r.get::<_, String>(8)?,
                "created_at": r.get::<_, String>(9)?,
                "reason": r.get::<_, Option<String>>(10)?,
            }))
        }).map_err(|e| e.to_string())?
          .collect::<Result<_, _>>().map_err(|e| e.to_string())?;
        drop(stmt);
        tx.execute(
            "DELETE FROM relations WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)",
            params![entity_id],
        ).map_err(|e| e.to_string())?;
        rows
    } else {
        Vec::new()
    };

    let cascaded_history: u64 = if purge_history {
        tx.execute(
            "DELETE FROM relations WHERE valid_to IS NOT NULL AND (from_id = ?1 OR to_id = ?1)",
            params![entity_id],
        ).map_err(|e| e.to_string())? as u64
    } else { 0 };

    tx.execute("DELETE FROM entities WHERE id = ?1", params![entity_id])
        .map_err(|e| e.to_string())?;

    tx.execute(
        "DELETE FROM entity_aliases WHERE alias_id = ?1 OR canonical_id = ?1",
        params![entity_id],
    ).map_err(|e| e.to_string())?;

    let _ = tx.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        params![entity_id, format!("{}_chunk_%", entity_id)],
    );

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "op": "delete_entity_by_id",
        "entity": {
            "id": entity_id,
            "entity_type": entity_type,
            "scope": scope,
            "properties_json": properties_json,
        },
        "cascaded_relations": cascaded_rels,
        "cascaded_history": cascaded_history,
        "force": force,
        "purge_history": purge_history,
    }).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, (1 + cascaded_rels.len() + cascaded_history as usize) as i64, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(DeleteEntityReport {
        entity_id: entity_id.to_string(),
        cascaded_relations: cascaded_rels.len() as u64,
        cascaded_history,
    })
}

pub fn load_relation_by_rowid(conn: &Connection, rowid: i64) -> Result<Relation, String> {
    let (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, props_json, reason):
        (String, String, String, String, String, Option<String>, f64, String, Option<String>) = conn.query_row(
            "SELECT from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, reason
             FROM relations WHERE id = ?1",
            params![rowid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        ).map_err(|e| format!("load relation {}: {}", rowid, e))?;
    let properties = serde_json::from_str(&props_json)
        .map_err(|e| format!("parse properties for rowid {}: {}", rowid, e))?;
    Ok(Relation { from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties, reason })
}

#[cfg(test)] mod tests;
