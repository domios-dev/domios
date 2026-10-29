use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::service::ServiceState;

#[derive(Deserialize)]
pub struct ArchiveBody {
    pub reason: String,
    #[serde(default)]
    pub changed_by: Option<String>,
}

pub fn archive_mem_item(
    svc: &ServiceState,
    id: &str,
    reason: &str,
    actor: &str,
) -> Result<Value, (StatusCode, String)> {
    svc.mem.archive(id, reason, actor).map_err(|e| {
        (
            super::super::mem_merge::merge_error_status(&e),
            e.into_message(),
        )
    })
}

pub fn validate_archive_reason(reason: &str) -> Result<String, (StatusCode, String)> {
    let reason = reason.trim();
    if reason.is_empty() || reason.len() > 500 || reason.chars().any(char::is_control) {
        return Err((
            StatusCode::BAD_REQUEST,
            "reason must be 1 to 500 characters without control characters".into(),
        ));
    }
    Ok(reason.to_owned())
}

#[cfg(test)]
pub(crate) fn archive_mem_item_conn(
    conn: &rusqlite::Connection,
    id: &str,
    reason: &str,
    actor: &str,
) -> Result<Value, (StatusCode, String)> {
    codeterm_mem::backend::sqlite::archive_mem_item_conn(conn, id, reason, actor)
        .map_err(|(status, message)| (StatusCode::from_u16(status).unwrap(), message))
}

#[cfg(test)]
mod tests;
