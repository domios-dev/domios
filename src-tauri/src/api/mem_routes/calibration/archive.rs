//! The one archive mechanism: an archived item keeps its row, body and edges,
//! only `kind` becomes `archived`, and the prior kind stays in history for restore.

use rusqlite::{params, Connection};

use crate::mem::store::{history_log, with_changed_by, HistorySnapshot};

pub(crate) const ARCHIVED_KIND: &str = "archived";
pub(crate) const RESTORE_CHANGE_TYPE: &str = "restore";

pub(crate) fn archive_item(
    conn: &Connection,
    id: &str,
    prev_kind: &str,
    actor: &str,
    change_type: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    with_changed_by(actor, || {
        let changed = conn
            .execute(
                "UPDATE mem_items SET kind = ?2, updated_at = ?3 WHERE id = ?1 AND kind != ?2",
                params![id, ARCHIVED_KIND, chrono::Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("archive update: {}", e))?;
        if changed == 0 {
            return Err(format!("archive {id}: item missing or already archived"));
        }
        let old = HistorySnapshot {
            kind: Some(prev_kind.to_string()),
            ..Default::default()
        };
        let new = HistorySnapshot {
            kind: Some(ARCHIVED_KIND.to_string()),
            ..Default::default()
        };
        history_log(conn, id, change_type, &old, &new, reason);
        Ok(())
    })
}

pub(crate) fn unarchive_item(
    conn: &Connection,
    id: &str,
    kind: &str,
    actor: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    with_changed_by(actor, || {
        let changed = conn
            .execute(
                "UPDATE mem_items SET kind = ?2, updated_at = ?3 WHERE id = ?1 AND kind = ?4",
                params![id, kind, chrono::Utc::now().to_rfc3339(), ARCHIVED_KIND],
            )
            .map_err(|e| format!("restore kind: {}", e))?;
        if changed == 0 {
            return Err(format!("restore {id}: item missing or not archived"));
        }
        let old = HistorySnapshot {
            kind: Some(ARCHIVED_KIND.to_string()),
            ..Default::default()
        };
        let new = HistorySnapshot {
            kind: Some(kind.to_string()),
            ..Default::default()
        };
        history_log(conn, id, RESTORE_CHANGE_TYPE, &old, &new, reason);
        Ok(())
    })
}
