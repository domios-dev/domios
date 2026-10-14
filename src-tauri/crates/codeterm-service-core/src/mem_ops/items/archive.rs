use axum::http::StatusCode;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::Value;

use crate::mem_ops::calibration::archive::archive_item;
use crate::service::ServiceState;

#[derive(Deserialize)]
pub struct ArchiveBody {
    pub reason: String,
    #[serde(default)]
    pub changed_by: Option<String>,
}

pub(crate) fn archive_mem_item(
    svc: &ServiceState,
    id: &str,
    reason: &str,
    actor: &str,
) -> Result<Value, (StatusCode, String)> {
    svc.mem
        .with_conn(|conn| archive_mem_item_conn(conn, id, reason, actor))
}

pub(crate) fn validate_archive_reason(reason: &str) -> Result<String, (StatusCode, String)> {
    let reason = reason.trim();
    if reason.is_empty() || reason.len() > 500 || reason.chars().any(char::is_control) {
        return Err((
            StatusCode::BAD_REQUEST,
            "reason must be 1 to 500 characters without control characters".into(),
        ));
    }
    Ok(reason.to_owned())
}

pub(crate) fn archive_mem_item_conn(
    conn: &rusqlite::Connection,
    id: &str,
    reason: &str,
    actor: &str,
) -> Result<Value, (StatusCode, String)> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let result = (|| {
        let item: Option<(String, bool)> = conn
            .query_row(
                "SELECT kind, is_pinned_wakeup FROM mem_items WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .optional()
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let Some((kind, pinned)) = item else {
            return Err((StatusCode::NOT_FOUND, format!("mem item {id} not found")));
        };
        if pinned {
            return Err((
                StatusCode::CONFLICT,
                "pinned items cannot be archived".into(),
            ));
        }
        if matches!(kind.as_str(), "skill" | "rule" | "preference") {
            return Err((
                StatusCode::CONFLICT,
                format!("{kind} items cannot be archived"),
            ));
        }
        if kind == "archived" {
            return Err((StatusCode::CONFLICT, "item is already archived".into()));
        }
        archive_item(conn, id, &kind, actor, "archive", Some(reason))
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        Ok(serde_json::json!({ "archived": id, "previous_kind": kind, "reason": reason }))
    })();
    match result {
        Ok(output) => conn
            .execute_batch("COMMIT")
            .map(|_| output)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
        Err(error) => {
            conn.execute_batch("ROLLBACK").ok();
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
