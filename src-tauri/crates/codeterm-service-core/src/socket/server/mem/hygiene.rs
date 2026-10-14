//! Socket handlers for reversible hygiene: merge, restore and the stale archive.
//! The logic lives in `mem_ops` so HTTP and socket cannot drift.

use axum::http::StatusCode;

use crate::mem_ops::calibration::stale::{stale_from_params, StaleParams};
use crate::mem_ops::items::archive::{archive_mem_item, validate_archive_reason, ArchiveBody};
use crate::mem_ops::items::RestoreBody;
use crate::mem_ops::mem_merge::{merge_items, restore_item, MergeBody, MergeError};
use crate::service::ServiceState;

use super::super::{get_string_param, JsonRpcError};
use super::{emit_activity, internal_error, invalid_params, not_found, parse_params};

pub(super) fn mem_merge(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let body: MergeBody = parse_params(params)?;
    let out = merge_items(svc, &body, body.worker_id.as_deref()).map_err(|e| match e {
        MergeError::NotFound(m) => not_found(m),
        MergeError::Internal(m) => internal_error(m),
        other => invalid_params(other.into_message()),
    })?;
    emit_activity(svc, "update", vec![body.keeper, body.lesser]);
    Ok(out)
}

pub(super) fn mem_restore(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let body: RestoreBody = parse_params(params)?;
    let out = restore_item(svc, &id, &body).map_err(|(status, message)| match status {
        StatusCode::NOT_FOUND => not_found(message),
        StatusCode::BAD_REQUEST => invalid_params(message),
        _ => internal_error(message),
    })?;
    emit_activity(svc, "update", vec![id]);
    Ok(out)
}

pub(super) fn mem_archive(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let body: ArchiveBody = parse_params(params)?;
    let reason =
        validate_archive_reason(&body.reason).map_err(|(_, message)| invalid_params(message))?;
    let actor = body
        .changed_by
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(crate::mem_ops::sanitize_actor)
        .unwrap_or_else(|| "user".to_string());
    let output =
        archive_mem_item(svc, &id, &reason, &actor).map_err(|(status, message)| match status {
            StatusCode::NOT_FOUND => not_found(message),
            StatusCode::BAD_REQUEST | StatusCode::CONFLICT => invalid_params(message),
            _ => internal_error(message),
        })?;
    emit_activity(svc, "update", vec![id]);
    Ok(output)
}

pub(super) fn mem_calibration_stale(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: StaleParams = parse_params(params)?;
    let hygiene = svc.configuration().memory.dream_hygiene;
    svc.mem
        .with_conn(|conn| stale_from_params(conn, &p, &hygiene))
        .map_err(internal_error)
}

pub(super) fn mem_area_merge(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let body: crate::mem_ops::area_merge::AreaMergeBody = parse_params(params)?;
    let out = crate::mem_ops::area_merge::merge_area(svc, &body, body.worker_id.as_deref())
        .map_err(invalid_params)?;
    if !body.dry_run {
        let ids = serde_json::from_value(out["item_ids"].clone()).unwrap_or_default();
        emit_activity(svc, "update", ids);
    }
    Ok(out)
}
