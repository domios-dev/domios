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

pub fn load_relation_by_rowid(conn: &Connection, rowid: i64) -> Result<Relation, String> {
    let (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, props_json):
        (String, String, String, String, String, Option<String>, f64, String) = conn.query_row(
            "SELECT from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json
             FROM relations WHERE id = ?1",
            params![rowid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
        ).map_err(|e| format!("load relation {}: {}", rowid, e))?;
    let properties = serde_json::from_str(&props_json)
        .map_err(|e| format!("parse properties for rowid {}: {}", rowid, e))?;
    Ok(Relation { from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties })
}

#[cfg(test)] mod tests;
