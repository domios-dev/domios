use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDelayedMessageBody {
    text: String,
    scheduled_at_ms: Option<u64>,
    created_by_tab: Option<String>,
    recurrence: Option<codeterm_service_core::delayed_send::recurrence::RecurrenceRule>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDelayedMessageBody {
    text: Option<String>,
    scheduled_at_ms: Option<u64>,
    stash: Option<bool>,
    recurrence: Option<codeterm_service_core::delayed_send::recurrence::RecurrenceRule>,
}

pub(crate) fn mutation_refusal_response(
    id: &str,
) -> Option<(
    StatusCode,
    Json<codeterm_service_core::delayed_send::SystemEntryRefusal>,
)> {
    codeterm_service_core::delayed_send::system_entry_refusal(id)
        .map(|body| (StatusCode::CONFLICT, Json(body)))
}

pub async fn list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<Vec<codeterm_service_core::delayed_send::DelayedMessageView>>, (StatusCode, String)>
{
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(s, m)| (s, m.to_string()))?;
    let messages = codeterm_service_core::delayed_send::list_views(&state.svc, Some(tab_id))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(messages))
}

pub async fn create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<CreateDelayedMessageBody>,
) -> Result<Json<codeterm_service_core::delayed_send::DelayedMessageView>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(s, m)| (s, m.to_string()))?;
    let msg = codeterm_service_core::delayed_send::create_message(
        &state.svc,
        tab_id,
        body.text,
        body.scheduled_at_ms,
        body.created_by_tab,
        body.recurrence,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(msg))
}

pub async fn system_heartbeats(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<Vec<codeterm_service_core::delayed_send::DelayedMessageView>>, (StatusCode, String)>
{
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(s, m)| (s, m.to_string()))?;
    codeterm_service_core::delayed_send::system_heartbeat_views(&state.svc, &tab_id)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateDelayedMessageBody>,
) -> Result<Json<codeterm_service_core::delayed_send::DelayedMessageView>, Response> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()).into_response())?;
    if let Some(refusal) = mutation_refusal_response(&id) {
        return Err(refusal.into_response());
    }
    let msg = codeterm_service_core::delayed_send::update_message(
        &state.svc,
        id,
        body.text,
        body.scheduled_at_ms,
        body.stash,
        body.recurrence,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e).into_response())?;
    Ok(Json(msg))
}

pub async fn delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<codeterm_service_core::delayed_send::DelayedMessageView>, Response> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()).into_response())?;
    if let Some(refusal) = mutation_refusal_response(&id) {
        return Err(refusal.into_response());
    }
    let msg = codeterm_service_core::delayed_send::delete_message(&state.svc, id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e).into_response())?;
    Ok(Json(msg))
}

pub async fn send_now(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, Response> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()).into_response())?;
    if let Some(refusal) = mutation_refusal_response(&id) {
        return Err(refusal.into_response());
    }
    codeterm_service_core::delayed_send::send_message_now(state.svc.clone(), id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e).into_response())?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod tests;
