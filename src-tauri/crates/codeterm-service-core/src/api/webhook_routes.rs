use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::events::WebhookRegistration;
use crate::api::routes::{check_auth, AppState};

pub async fn list_webhooks(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let hooks = state.svc.events.list_webhooks();
    Ok(Json(serde_json::to_value(hooks).unwrap()))
}

#[derive(Deserialize)]
pub(crate) struct RegisterWebhookBody {
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
    headers: HeaderMap,
    Json(body): Json<RegisterWebhookBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
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
    state.svc.events.register_webhook(reg);
    Ok(Json(serde_json::json!({"id": id, "once": body.once})))
}

pub async fn unregister_webhook(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.events.unregister_webhook(&id);
    Ok(Json(serde_json::json!({"ok": true})))
}
