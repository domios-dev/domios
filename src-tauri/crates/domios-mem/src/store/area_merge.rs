use std::{collections::BTreeMap, path::Path};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    current_changed_by, get_item_history, insert, update, MemKind, MemScope, MemUpdate, NewMemItem,
};

pub const DREAMER_AREA_MERGE_CAP: usize = 3;
const MERGE_PREFIX: &str = "area_merge=";

#[derive(Debug, Serialize)]
pub struct AreaMergeOutcome {
    pub from: String,
    pub into: String,
    pub dry_run: bool,
    pub counts: BTreeMap<String, usize>,
    pub item_ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct AreaChange {
    from: Option<String>,
    into: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dream_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operation: Option<String>,
}

pub fn validate_areas(from: &str, into: &str) -> Result<(), String> {
    for name in [from, into] {
        if name.trim().is_empty() || name != name.trim() || name.chars().any(char::is_control) {
            return Err("area must be nonempty, trimmed and contain no control characters".into());
        }
        if name.starts_with('_') {
            return Err("reserved system areas cannot be merged".into());
        }
    }
    if from == into {
        return Err("source and destination areas must differ".into());
    }
    Ok(())
}

pub(super) fn update_area(conn: &Connection, id: &str, into: &str) -> Result<bool, String> {
    let previous: Option<Option<String>> = conn
        .query_row("SELECT area FROM mem_items WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(from) = previous else {
        return Ok(false);
    };
    if from.as_deref() == Some(into) {
        return Ok(false);
    }
    conn.execute_batch("SAVEPOINT mem_area_update")
        .map_err(|e| e.to_string())?;
    let applied = (|| -> Result<(), String> {
        conn.execute(
            "UPDATE mem_items SET area=?1, updated_at=?2, seed_fingerprint=NULL WHERE id=?3",
            params![into, chrono::Utc::now().to_rfc3339(), id],
        )
        .map_err(|e| e.to_string())?;
        let reason = format!(
            "area_update={}",
            serde_json::to_string(&AreaChange {
                from,
                into: into.into(),
                dream_run: None,
                operation: None,
            })
            .map_err(|e| e.to_string())?
        );
        conn.execute("INSERT INTO mem_item_history (item_id, changed_at, changed_by, change_type, reason) VALUES (?1, ?2, ?3, 'update', ?4)",
            params![id, chrono::Utc::now().to_rfc3339(), current_changed_by(), reason]).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if applied.is_err() {
        conn.execute_batch("ROLLBACK TO mem_area_update")
            .map_err(|e| e.to_string())?;
    }
    conn.execute_batch("RELEASE mem_area_update")
        .map_err(|e| e.to_string())?;
    applied?;
    Ok(true)
}

pub fn merge_area(
    conn: &Connection,
    blobs: &Path,
    from: &str,
    into: &str,
    dry_run: bool,
    dream_run: Option<&str>,
) -> Result<AreaMergeOutcome, String> {
    validate_areas(from, into)?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let rows: Vec<(String, String)> = {
        let mut stmt = tx
            .prepare("SELECT id, kind FROM mem_items WHERE area=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([from], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };
    if rows.is_empty() {
        return Err(format!("source area {from} is empty or does not exist"));
    }
    let mut counts = BTreeMap::new();
    for (_, kind) in &rows {
        *counts.entry(kind.clone()).or_insert(0) += 1;
    }
    let outcome = AreaMergeOutcome {
        from: from.into(),
        into: into.into(),
        dry_run,
        counts,
        item_ids: rows.iter().map(|(id, _)| id.clone()).collect(),
    };
    if dry_run {
        return Ok(outcome);
    }
    let run_source = dream_run.map(|run| format!("dreamer-area-merge:{run}"));
    if let Some(run) = dream_run {
        let count: usize = tx.query_row(
            "SELECT COUNT(DISTINCT json_extract(substr(reason,12), '$.operation'))
             FROM mem_item_history WHERE substr(reason,1,11) = 'area_merge='
             AND json_extract(CASE WHEN json_valid(substr(reason,12)) THEN substr(reason,12) ELSE '{}' END, '$.dream_run') = ?1",
            [run], |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        if count >= DREAMER_AREA_MERGE_CAP {
            return Err("dream run area merge cap reached (3)".into());
        }
    }
    let reason = format!(
        "{MERGE_PREFIX}{}",
        serde_json::to_string(&AreaChange {
            from: Some(from.into()),
            into: into.into(),
            dream_run: dream_run.map(str::to_string),
            operation: Some(ulid::Ulid::new().to_string()),
        })
        .map_err(|e| e.to_string())?
    );
    for (id, _) in &rows {
        if !update(
            &tx,
            id,
            &MemUpdate {
                area: Some(into.into()),
                ..Default::default()
            },
            blobs,
        )? {
            return Err(format!("area update failed for {id}"));
        }
        tx.execute("UPDATE mem_item_history SET reason=?1 WHERE id=(SELECT MAX(id) FROM mem_item_history WHERE item_id=?2)",
            params![reason, id]).map_err(|e| e.to_string())?;
    }
    if let Some(source) = run_source {
        let summary = format!("Area merge {from:?} -> {into:?}: {} items; undo each with codeterm mem restore <id> --version <history-row>.", rows.len());
        let diary_id = insert(
            &tx,
            &NewMemItem {
                kind: MemKind::Daily,
                scope: MemScope::Global,
                title: format!("Area merge {}", ulid::Ulid::new()),
                summary: Some(summary.clone()),
                area: Some("_dreams".into()),
                tags: vec!["dream-diary".into()],
                body: summary,
                source: Some(source),
                importance: 2.0,
                source_path: None,
                pinned: false,
            },
            blobs,
        )?;
        if get_item_history(&tx, &diary_id)?.is_empty() {
            return Err("area merge diary audit was not recorded".into());
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(outcome)
}

pub fn restore_area(
    conn: &Connection,
    blobs: &Path,
    id: &str,
    version: Option<i64>,
) -> Result<Option<String>, String> {
    let history = get_item_history(conn, id)?;
    let row = match version {
        Some(version) => history.iter().find(|h| h.id == version),
        None => history.last(),
    };
    let Some(encoded) = row
        .and_then(|h| h.reason.as_deref())
        .and_then(|r| r.strip_prefix(MERGE_PREFIX))
    else {
        return Ok(None);
    };
    let change: AreaChange = serde_json::from_str(encoded).map_err(|e| e.to_string())?;
    let from = change.from.ok_or("area merge history has no source")?;
    if !update(
        conn,
        id,
        &MemUpdate {
            area: Some(from.clone()),
            ..Default::default()
        },
        blobs,
    )? {
        return Err("area restore made no change; item missing or already restored".into());
    }
    Ok(Some(from))
}

#[cfg(test)]
mod tests;
