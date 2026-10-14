use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use super::recording::record_hook_post;
use super::{header_str, resolve_hook_tab};
use crate::routes::AppState;

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
) -> Result<axum::response::Response, (StatusCode, String)> {
    let started = std::time::Instant::now();
    let tab_id = resolve_hook_tab(&state, &raw_tab_id, "provider_event")?;
    if let Some(response) = super::hook_authority::telemetry_response(&tab_id, "provider_event") {
        return Ok(axum::response::IntoResponse::into_response(response));
    }
    let provider = header_str(&headers, "x-codeterm-agent-provider").ok_or((
        StatusCode::UNAUTHORIZED,
        "provider identity missing".to_string(),
    ))?;
    let nonce = header_str(&headers, "x-codeterm-binding-nonce").ok_or((
        StatusCode::UNAUTHORIZED,
        "binding nonce missing".to_string(),
    ))?;
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
    let raw_event = serde_json::json!({"session_id": &body.session_id, "event": &body.event});
    record_hook_post(&state, &tab_id, "provider_event", Some(&raw_event));
    let event_json = serde_json::to_string(&body.event).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "provider event is not valid JSON".to_string(),
        )
    })?;
    let registry = state.svc.agent_registry.clone();
    let provider_id = provider.to_string();
    let events =
        tokio::task::spawn_blocking(move || registry.detect_events(&provider_id, &event_json))
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let turn_error =
        events.contains(&codeterm_service_core::agent::detector::AgentEvent::TurnError);
    let turn_complete =
        events.contains(&codeterm_service_core::agent::detector::AgentEvent::TaskComplete);
    let turn_started =
        events.contains(&codeterm_service_core::agent::detector::AgentEvent::TurnStarted);
    if turn_error || turn_complete {
        let registry = state.svc.agent_registry.clone();
        let provider = provider.to_string();
        let session = body.session_id.clone();
        let cwd = state
            .svc
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.cwd.clone())
            .unwrap_or_default();
        let svc = state.svc.clone();
        let target = tab_id.clone();
        let binding_nonce = nonce.to_string();
        tokio::task::spawn_blocking(move || {
            let records = registry
                .with_provider(&provider, |provider| {
                    provider
                        .read_structured_chat(&cwd, &session, None)
                        .observations
                })
                .unwrap_or_default();
            codeterm_service_core::service::provider_signals::apply_structured(
                &svc,
                &target,
                &session,
                &binding_nonce,
                &records,
            );
        })
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }
    if turn_error {
        if pty_id.is_none() || !state.svc.mark_agent_turn_error(&tab_id) {
            return Err((
                StatusCode::CONFLICT,
                "provider error tab has no PTY status".to_string(),
            ));
        }
    } else if turn_complete {
        let has_error = state
            .svc
            .mux
            .read()
            .get_tab(&tab_id)
            .is_some_and(|tab| tab.provider_signals.api_error.is_some());
        let marked = if has_error {
            state.svc.mark_agent_turn_error(&tab_id)
        } else {
            state.svc.mark_agent_turn_complete(&tab_id)
        };
        if pty_id.is_none() || !marked {
            return Err((
                StatusCode::CONFLICT,
                "provider idle tab has no PTY status".to_string(),
            ));
        }
        state.svc.schedule_session_model_refresh_after_stop(&tab_id);
        state
            .svc
            .staged_model_turn_completed(&tab_id, &body.session_id, nonce);
    } else if turn_started && (pty_id.is_none() || !state.svc.mark_agent_turn_started(&tab_id)) {
        return Err((
            StatusCode::CONFLICT,
            "provider active tab has no PTY status".to_string(),
        ));
    }
    let response = Json(serde_json::json!({
        "ok": true,
        "turn_error": turn_error,
        "turn_complete": turn_complete,
        "turn_started": turn_started,
    }));
    log::info!(
        "[hook/timing] tab={tab_id} route=provider_event phase=response_ready elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(if turn_started || turn_complete || turn_error {
        super::reconciliation::after_response(response, state.svc, tab_id, "provider_event")
    } else {
        axum::response::IntoResponse::into_response(response)
    })
}
