use rusqlite::{params, Connection};
use serde_json::{Map, Value};

use crate::mem::graph::Relation;

use super::types::{DuplicateRelationGroup, MergeReport, RelationColumn, RelationMember};

pub fn find_duplicate_relations(
    conn: &Connection,
    group_by: &[RelationColumn],
    limit: usize,
) -> Result<Vec<DuplicateRelationGroup>, String> {
    if group_by.is_empty() {
        return Err("group_by must contain at least one column".into());
    }
    let cols = group_by
        .iter()
        .map(|c| c.sql())
        .collect::<Vec<_>>()
        .join(",");

    let sql = format!(
        "SELECT {cols}, GROUP_CONCAT(id || '~' || relation_type, '|') AS members, COUNT(*) AS c
         FROM relations
         WHERE valid_to IS NULL
         GROUP BY {cols}
         HAVING c > 1
         ORDER BY c DESC
         LIMIT ?1",
        cols = cols
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(params![limit as i64])
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut key = Map::<String, Value>::new();
        for (i, col) in group_by.iter().enumerate() {
            let v: Option<String> = row.get(i).map_err(|e| e.to_string())?;
            key.insert(
                col.sql().into(),
                v.map(Value::String).unwrap_or(Value::Null),
            );
        }
        let members_csv: String = row.get(group_by.len()).map_err(|e| e.to_string())?;
        let count: i64 = row.get(group_by.len() + 1).map_err(|e| e.to_string())?;
        let members: Vec<RelationMember> = members_csv
            .split('|')
            .map(|tok| {
                let (id_s, rel) = tok
                    .split_once('~')
                    .ok_or_else(|| format!("malformed member token '{}'", tok))?;
                let rowid = id_s
                    .parse::<i64>()
                    .map_err(|e| format!("parse rowid '{}': {}", id_s, e))?;
                Ok(RelationMember {
                    rowid,
                    relation_type: rel.to_string(),
                })
            })
            .collect::<Result<_, String>>()?;
        let rowids: Vec<i64> = members.iter().map(|m| m.rowid).collect();
        let first_rowid = *rowids
            .first()
            .ok_or("empty rowid group from GROUP_CONCAT")?;
        let sample = load_relation_by_rowid(conn, first_rowid)?;
        out.push(DuplicateRelationGroup {
            key: Value::Object(key),
            rowids,
            members,
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

    let placeholders = std::iter::repeat("?")
        .take(rowids.len())
        .collect::<Vec<_>>()
        .join(",");
    let payload_sql = format!(
        "SELECT id, from_id, relation_type, to_id, scope, valid_from, valid_to,
                confidence, properties_json, created_at
         FROM relations WHERE id IN ({})",
        placeholders
    );
    let mut stmt = tx.prepare(&payload_sql).map_err(|e| e.to_string())?;
    let bind: Vec<&dyn rusqlite::ToSql> =
        rowids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let payload: Vec<Value> = stmt
        .query_map(rusqlite::params_from_iter(bind.iter()), |r| {
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
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let del_sql = format!("DELETE FROM relations WHERE id IN ({})", placeholders);
    let bind: Vec<&dyn rusqlite::ToSql> =
        rowids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let deleted = tx
        .execute(&del_sql, rusqlite::params_from_iter(bind.iter()))
        .map_err(|e| e.to_string())? as u64;

    let now = chrono::Utc::now().to_rfc3339();
    let details =
        serde_json::json!({"op": "delete_relations_by_rowids", "payload": payload}).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, deleted as i64, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(deleted)
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
    let placeholders = std::iter::repeat("?")
        .take(all_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let bind: Vec<&dyn rusqlite::ToSql> =
        all_ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();

    let mut stmt = tx
        .prepare(&format!(
            "SELECT id, confidence, properties_json FROM relations WHERE id IN ({})",
            placeholders
        ))
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, f64, String)> = stmt
        .query_map(rusqlite::params_from_iter(bind.iter()), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    if rows.len() != all_ids.len() {
        return Err(format!(
            "expected {} rows, got {} — some rowids do not exist",
            all_ids.len(),
            rows.len()
        ));
    }

    let keep_row = rows
        .iter()
        .find(|(id, _, _)| *id == keep)
        .ok_or_else(|| format!("keep row {} not found", keep))?;
    let mut keep_props: Map<String, Value> = serde_json::from_str::<Value>(&keep_row.2)
        .map_err(|e| format!("parse keep props (rowid {}): {}", keep, e))?
        .as_object()
        .cloned()
        .unwrap_or_default();

    let mut max_conf = keep_row.1;
    for (id, conf, props_json) in &rows {
        if *id == keep {
            continue;
        }
        if *conf > max_conf {
            max_conf = *conf;
        }
        let other: Value = serde_json::from_str(props_json)
            .map_err(|e| format!("parse merge props (rowid {}): {}", id, e))?;
        if let Some(obj) = other.as_object() {
            for (k, v) in obj {
                keep_props.entry(k.clone()).or_insert(v.clone());
            }
        }
    }

    let merged_props_json =
        serde_json::to_string(&Value::Object(keep_props.clone())).map_err(|e| e.to_string())?;

    tx.execute(
        "UPDATE relations SET confidence = ?1, properties_json = ?2 WHERE id = ?3",
        params![max_conf, merged_props_json, keep],
    )
    .map_err(|e| e.to_string())?;

    let merge_placeholders = std::iter::repeat("?")
        .take(merge.len())
        .collect::<Vec<_>>()
        .join(",");
    let merge_bind: Vec<&dyn rusqlite::ToSql> =
        merge.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let deleted = tx
        .execute(
            &format!("DELETE FROM relations WHERE id IN ({})", merge_placeholders),
            rusqlite::params_from_iter(merge_bind.iter()),
        )
        .map_err(|e| e.to_string())? as i64;

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "op": "merge_relations",
        "kept": keep,
        "merged_in": merge,
        "final_confidence": max_conf,
        "final_properties": Value::Object(keep_props),
    })
    .to_string();
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
    Ok(Relation {
        from_id,
        relation_type,
        to_id,
        scope,
        valid_from,
        valid_to,
        confidence,
        properties,
        reason,
    })
}
