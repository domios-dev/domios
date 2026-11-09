use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::events::WebhookRegistration;
use crate::routes::AppState;

pub async fn list_webhooks(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let hooks = state.svc.events.list_webhooks();
    Ok(Json(serde_json::to_value(hooks).unwrap()))
}

#[derive(Deserialize)]
pub struct RegisterWebhookBody {
    url: String,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    tab_id: Option<String>,
    #[serde(default)]
    once: bool,
    payload_extra: Option<serde_json::Value>,
}

pub async fn register_webhook(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(body): Json<RegisterWebhookBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let id = uuid::Uuid::new_v4().to_string();
    let reg = WebhookRegistration {
        id: id.clone(),
        url: body.url,
        events: body.events,
        headers: body.headers,
        tab_id: body.tab_id,
        once: body.once,
        payload_extra: body.payload_extra,
    };
    state
        .svc
        .events
        .register_webhook(reg)
        .map_err(|error| (StatusCode::TOO_MANY_REQUESTS, error))?;
    Ok(Json(serde_json::json!({"id": id, "once": body.once})))
}

pub async fn unregister_webhook(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    state.svc.events.unregister_webhook(&id);
    Ok(Json(serde_json::json!({"ok": true})))
}
