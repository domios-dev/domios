//! Delete and batch-forget routes, and the honest reporting of what they
//! cancelled. A revoked extraction generation whose cancellation is never
//! surfaced looks identical to one that silently ran to completion, so these
//! routes name the generations they revoked.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::{check_auth, AppState};

use super::super::{changed_by_from_headers, emit_activity};

pub async fn mem_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let changed_by = changed_by_from_headers(&headers);
    let before = state.svc.mem.extraction_authority().open_generations(&id);
    let removed = delete_item(state.svc.mem.clone(), id.clone(), changed_by)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "delete", vec![id.clone()]);
    Ok(Json(serde_json::json!({
        "deleted": removed,
        "cancelled_extractions": cancelled_report(&state, &before, removed),
    })))
}

pub async fn mem_forget_area(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(area): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let outcome = delete_area(state.svc.mem.clone(), area.clone())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "delete", vec![format!("area:{}", area)]);
    Ok(Json(serde_json::json!({
        "deleted": outcome.deleted,
        "area": area,
        "cancelled_extractions": outcome.cancelled_extractions,
    })))
}

fn cancelled_report(
    state: &AppState,
    generations: &[codeterm_mem::GenerationId],
    removed: bool,
) -> Vec<serde_json::Value> {
    if !removed {
        return Vec::new();
    }
    generations
        .iter()
        .map(|g| {
            serde_json::json!({
                "generation": g.token(),
                "source_id": g.source_id,
                "revoked": state.svc.mem.extraction_authority().is_revoked(g),
            })
        })
        .collect()
}

async fn delete_item(
    mem: std::sync::Arc<codeterm_service_core::mem::MemManager>,
    id: String,
    changed_by: String,
) -> Result<bool, String> {
    codeterm_service_core::service::blocking::run(move || {
        codeterm_service_core::mem::store::with_changed_by(&changed_by, || mem.delete(&id))
    })
    .await
}

async fn delete_area(
    mem: std::sync::Arc<codeterm_service_core::mem::MemManager>,
    area: String,
) -> Result<codeterm_mem::AreaForgetOutcome, String> {
    codeterm_service_core::service::blocking::run(move || mem.delete_by_area(&area)).await
}

#[cfg(test)]
mod tests;
