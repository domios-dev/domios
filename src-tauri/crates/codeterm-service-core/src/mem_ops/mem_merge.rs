use axum::http::StatusCode;
use chrono::Utc;

use super::items::{pick_restore_body, RestoreBody};
use super::sanitize_actor;
use crate::mem::store::{self, with_changed_by, MemUpdate};
use crate::service::ServiceState;

pub use codeterm_mem::curation::merge::*;

pub(crate) fn merge_error_status(err: &MergeError) -> StatusCode {
    StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Shared by the HTTP and socket transports. Config and the worker registry
/// are read before the mem connection lock is taken.
pub fn merge_items(
    svc: &ServiceState,
    body: &MergeBody,
    worker_id: Option<&str>,
) -> Result<serde_json::Value, MergeError> {
    let intent = worker_id.and_then(|wid| svc.workers.get(wid).map(|w| w.intent));
    let actor = merge_actor(intent.as_deref(), body.changed_by.as_deref());
    let merge_cap = svc.configuration().memory.dream_hygiene.merge_cap;
    let req = MergeRequest {
        keeper: body.keeper.trim(),
        lesser: body.lesser.trim(),
        body: body.body.as_deref(),
        conflict: body.conflict,
    };
    let out = svc.mem.with_conn(|conn| {
        run_merge(
            conn,
            svc.mem.blobs_dir(),
            &req,
            &actor,
            merge_cap,
            Utc::now(),
        )
    })?;
    if out["body_updated"] == true {
        svc.mem.refresh_embedding(req.keeper);
    }
    Ok(out)
}

/// `mem restore` for both transports: an archived item gets its kind back
/// (and the body of `version` when given); otherwise the latest body change is undone.
pub fn restore_item(
    svc: &ServiceState,
    id: &str,
    body: &RestoreBody,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let internal = |e: String| (StatusCode::INTERNAL_SERVER_ERROR, e);
    let history = svc.mem.get_item_history(id).map_err(internal)?;
    if history.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no history for {}", id)));
    }
    let actor = body
        .changed_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_actor)
        .unwrap_or_else(|| "restore".to_string());

    let area = with_changed_by(&actor, || {
        svc.mem.with_conn(|conn| {
            store::area_merge::restore_area(conn, svc.mem.blobs_dir(), id, body.version)
        })
    })
    .map_err(internal)?;
    if let Some(area) = area {
        svc.mem.refresh_embedding(id);
        return Ok(serde_json::json!({ "restored": id, "area": area }));
    }

    if let Some(kind) = pick_restore_kind(&history) {
        let restored_body = body
            .version
            .map(|v| pick_restore_body(&history, Some(v)))
            .transpose()?;
        let out = svc
            .mem
            .with_conn(|conn| {
                apply_kind_restore(
                    conn,
                    svc.mem.blobs_dir(),
                    id,
                    &kind,
                    restored_body.as_deref(),
                    &actor,
                )
            })
            .map_err(internal)?;
        svc.mem.refresh_embedding(id);
        return Ok(out);
    }

    let restored_body = pick_restore_body(&history, body.version)?;
    let patch = MemUpdate {
        body: Some(restored_body.clone()),
        ..Default::default()
    };
    let updated = with_changed_by(&actor, || svc.mem.update(id, patch)).map_err(internal)?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id)));
    }
    Ok(serde_json::json!({ "restored": id, "body_len": restored_body.len() }))
}
