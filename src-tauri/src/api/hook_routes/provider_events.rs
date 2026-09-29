use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::AppState;
use super::{header_str, resolve_hook_tab};

#[derive(serde::Deserialize)]
pub(crate) struct ProviderEventBody {
    session_id: String,
    event: serde_json::Value,
}

pub(crate) async fn hook_provider_event(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ProviderEventBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = resolve_hook_tab(&state, &raw_tab_id, "provider_event")?;
    let provider = header_str(&headers, "x-codeterm-agent-provider")
        .ok_or((StatusCode::UNAUTHORIZED, "provider identity missing".to_string()))?;
    let nonce = header_str(&headers, "x-codeterm-binding-nonce")
        .ok_or((StatusCode::UNAUTHORIZED, "binding nonce missing".to_string()))?;
    let (tab_provider, session_id, expected_nonce, acknowledged_nonce, pty_id) = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .map(|tab| {
            (
                tab.agent_type.clone(),
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
                tab.reported_session_binding_nonce.clone(),
                tab.pty_id.clone(),
            )
        })
        .unwrap_or_default();
    if tab_provider.as_deref() != Some(provider)
        || session_id.as_deref() != Some(body.session_id.as_str())
        || expected_nonce.as_deref() != Some(nonce)
        || acknowledged_nonce.as_deref() != Some(nonce)
    {
        return Err((
            StatusCode::CONFLICT,
            "provider event does not match the tab's claimed session".to_string(),
        ));
    }
    let event_json = serde_json::to_string(&body.event)
        .map_err(|_| (StatusCode::BAD_REQUEST, "provider event is not valid JSON".to_string()))?;
    let registry = state.svc.agent_registry.clone();
    let provider_id = provider.to_string();
    let events = tokio::task::spawn_blocking(move || registry.detect_events(&provider_id, &event_json))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let turn_error = events.contains(&crate::agent::detector::AgentEvent::TurnError);
    let turn_complete = events.contains(&crate::agent::detector::AgentEvent::TaskComplete);
    let turn_started = events.contains(&crate::agent::detector::AgentEvent::TurnStarted);
    if turn_error {
        if pty_id.is_none() || !state.svc.mark_agent_turn_error(&tab_id) {
            return Err((
                StatusCode::CONFLICT,
                "provider error tab has no PTY status".to_string(),
            ));
        }
    } else if turn_complete {
        if pty_id.is_none() || !state.svc.mark_agent_turn_complete(&tab_id) {
            return Err((
                StatusCode::CONFLICT,
                "provider idle tab has no PTY status".to_string(),
            ));
        }
    } else if turn_started {
        if pty_id.is_none() || !state.svc.mark_agent_turn_started(&tab_id) {
            return Err((
                StatusCode::CONFLICT,
                "provider active tab has no PTY status".to_string(),
            ));
        }
    }
    Ok(Json(serde_json::json!({
        "ok": true,
        "turn_error": turn_error,
        "turn_complete": turn_complete,
        "turn_started": turn_started,
    })))
}
