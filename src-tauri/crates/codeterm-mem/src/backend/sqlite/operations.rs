use crate::backend::WriteContext;
use crate::curation::merge::MergeError;
use rusqlite::{Connection, OptionalExtension};

pub(crate) fn archive_item(
    ctx: &WriteContext,
    conn: &Connection,
    id: &str,
    reason: &str,
    actor: &str,
) -> Result<serde_json::Value, MergeError> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| MergeError::Internal(e.to_string()))?;
    let result = (|| {
        let item: Option<(String, bool)> = conn
            .query_row(
                "SELECT kind, is_pinned_wakeup FROM mem_items WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .optional()
            .map_err(|e| MergeError::Internal(e.to_string()))?;
        let Some((kind, pinned)) = item else {
            return Err(MergeError::NotFound(format!("mem item {id} not found")));
        };
        if pinned {
            return Err(MergeError::Refused(
                "pinned items cannot be archived".into(),
            ));
        }
        if matches!(kind.as_str(), "skill" | "rule" | "preference") {
            return Err(MergeError::Refused(format!(
                "{kind} items cannot be archived"
            )));
        }
        if kind == "archived" {
            return Err(MergeError::Refused("item is already archived".into()));
        }
        crate::curation::calibration::archive::archive_item_with_context(
            ctx,
            conn,
            id,
            &kind,
            actor,
            "archive",
            Some(reason),
        )
        .map_err(MergeError::Internal)?;
        Ok(serde_json::json!({ "archived": id, "previous_kind": kind, "reason": reason }))
    })();
    match result {
        Ok(output) => conn
            .execute_batch("COMMIT")
            .map(|_| output)
            .map_err(|e| MergeError::Internal(e.to_string())),
        Err(error) => {
            conn.execute_batch("ROLLBACK").ok();
            Err(error)
        }
    }
}

pub(super) fn graph_type_counts(conn: &Connection) -> (Vec<(String, u64)>, Vec<(String, u64)>) {
    let query = |sql: &str| {
        conn.prepare(sql)
            .and_then(|mut stmt| {
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
    };
    let entities = query("SELECT entity_type, COUNT(*) FROM entities GROUP BY entity_type ORDER BY COUNT(*) DESC LIMIT 20");
    let relations = query("SELECT relation_type, COUNT(*) FROM relations GROUP BY relation_type ORDER BY COUNT(*) DESC LIMIT 30");
    (entities, relations)
}

pub fn diary_page(
    conn: &rusqlite::Connection,
    blobs_dir: &std::path::Path,
    offset: usize,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_items WHERE kind = 'daily' AND area = '_dreams'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("count dream diary: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT id FROM mem_items
             WHERE kind = 'daily' AND area = '_dreams'
             ORDER BY created_at DESC, id DESC
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| format!("prepare dream diary: {e}"))?;
    let ids = stmt
        .query_map(rusqlite::params![limit as i64, offset as i64], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| format!("query dream diary: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read dream diary: {e}"))?;
    let items = ids
        .iter()
        .map(|id| {
            crate::store::get(conn, id, blobs_dir)
                .and_then(|item| item.ok_or_else(|| format!("dream diary item {id} disappeared")))
                .and_then(|item| serde_json::to_value(item).map_err(|e| e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let next_offset = (offset + items.len() < total as usize).then_some(offset + items.len());
    Ok(serde_json::json!({
        "items": items,
        "total": total,
        "next_offset": next_offset,
    }))
}

pub fn bump_on_get(conn: &rusqlite::Connection, kind: &str, id: &str) {
    if !matches!(kind, "skill" | "lesson" | "rule" | "fact" | "preference") {
        return;
    }
    if let Err(e) = codeterm_mem_store::graph::bump_evidence_count(conn, id) {
        log::debug!("[mem_get] evidence bump failed for {}: {}", id, e);
    }
}
