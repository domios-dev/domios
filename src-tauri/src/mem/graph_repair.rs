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
        let rowids: Vec<i64> = rowids_csv.split(',').filter_map(|s| s.parse().ok()).collect();
        let sample = load_relation_by_rowid(conn, rowids[0])?;
        out.push(DuplicateRelationGroup {
            key: Value::Object(key),
            rowids,
            count: count as usize,
            sample,
        });
    }
    Ok(out)
}

fn load_relation_by_rowid(conn: &Connection, rowid: i64) -> Result<Relation, String> {
    conn.query_row(
        "SELECT from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json
         FROM relations WHERE id = ?1",
        params![rowid],
        |r| {
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
        },
    ).map_err(|e| format!("load relation {}: {}", rowid, e))
}

#[cfg(test)] mod tests;
