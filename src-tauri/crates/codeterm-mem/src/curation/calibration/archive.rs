//! The one archive mechanism: an archived item keeps its row, body and edges,
//! only `kind` becomes `archived`, and the prior kind stays in history for restore.

use rusqlite::{params, Connection};

use crate::store::HistorySnapshot;

pub const ARCHIVED_KIND: &str = "archived";
pub const RESTORE_CHANGE_TYPE: &str = "restore";

pub fn archive_item(
    conn: &Connection,
    id: &str,
    prev_kind: &str,
    actor: &str,
    change_type: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    archive_item_with_context(
        &crate::MemManager::write_context(),
        conn,
        id,
        prev_kind,
        actor,
        change_type,
        reason,
    )
}

pub fn archive_item_with_context(
    ctx: &crate::backend::WriteContext,
    conn: &Connection,
    id: &str,
    prev_kind: &str,
    actor: &str,
    change_type: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    {
        let ctx = &ctx.with_changed_by(actor);
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
        crate::store::history_log_with_context(ctx, conn, id, change_type, &old, &new, reason);
        Ok(())
    }
}

pub fn unarchive_item(
    conn: &Connection,
    id: &str,
    kind: &str,
    actor: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    unarchive_item_with_context(
        &crate::MemManager::write_context(),
        conn,
        id,
        kind,
        actor,
        reason,
    )
}

pub fn unarchive_item_with_context(
    ctx: &crate::backend::WriteContext,
    conn: &Connection,
    id: &str,
    kind: &str,
    actor: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    if let Some(author) = crate::store::item_author(conn, id)? {
        crate::store::guard_instruction(&author, kind, false)?;
    }
    crate::store::guard_instruction(&ctx.author, kind, false)?;
    {
        let ctx = &ctx.with_changed_by(actor);
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
        crate::store::history_log_with_context(
            ctx,
            conn,
            id,
            RESTORE_CHANGE_TYPE,
            &old,
            &new,
            reason,
        );
        Ok(())
    }
}
