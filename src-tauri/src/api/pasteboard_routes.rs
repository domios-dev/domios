//! REST endpoints for the cross-mesh pastebin (#98).
//!
//! All endpoints require the standard auth token. Mesh peers reach these
//! via the existing /api/mesh/relay envelope, so no separate mesh-specific
//! plumbing is required.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::pasteboard::PutParams;

#[derive(Deserialize)]
pub(crate) struct PasteCreateBody {
    body: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    ttl_ms: Option<u64>,
    #[serde(default)]
    pop_on_read: Option<bool>,
}

pub async fn paste_create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<PasteCreateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let entry = state
        .svc
        .pasteboard
        .put(PutParams {
            body: body.body,
            content_type: body.content_type,
            ttl_ms: body.ttl_ms,
            pop_on_read: body.pop_on_read,
            source_peer: None,
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "id": entry.id,
        "expires_at": entry.expires_at,
        "content_type": entry.content_type,
        "pop_on_read": entry.pop_on_read,
    })))
}

#[derive(Deserialize)]
pub(crate) struct PasteGetQuery {
    #[serde(default)]
    pop: Option<bool>,
}

pub async fn paste_get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<PasteGetQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let entry = state
        .svc
        .pasteboard
        .get(&id, query.pop.unwrap_or(false))
        .ok_or((StatusCode::NOT_FOUND, format!("paste not found: {}", id)))?;
    // Body returned as text. Binary payloads are not the canonical use case
    // — `String::from_utf8_lossy` keeps the response printable without
    // pretending to be lossless. If/when binary support lands, switch to
    // a separate base64 field.
    Ok(Json(serde_json::json!({
        "id": entry.id,
        "body": String::from_utf8_lossy(&entry.body),
        "content_type": entry.content_type,
        "created_at": entry.created_at,
        "expires_at": entry.expires_at,
        "pop_on_read": entry.pop_on_read,
    })))
}

pub async fn paste_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    state
        .svc
        .pasteboard
        .remove(&id)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn paste_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let entries = state.svc.pasteboard.list();
    Ok(Json(serde_json::to_value(entries).unwrap()))
}
