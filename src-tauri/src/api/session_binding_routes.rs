use axum::{
    extract::{Path, State as AxumState},
    http::StatusCode,
    Json,
};

use super::hook_routes::{
    bind_reported_session, finish_session_binding, SessionBindError, SessionStartClaim,
};
use super::routes::{resolve_tab, AppState};

#[derive(serde::Deserialize)]
pub(crate) struct RebindSessionRequest {
    session_id: String,
}

pub(crate) async fn binding_state(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<crate::service::SessionBindingState>, (StatusCode, String)> {
    let tab_id = resolve_tab(&state, &raw_tab_id)
        .map_err(|(status, message)| (status, message.to_string()))?;
    state
        .svc
        .session_binding_state(&tab_id)
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Tab not found".to_string()))
}

pub(crate) async fn rebind_session(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    Json(request): Json<RebindSessionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = resolve_tab(&state, &raw_tab_id)
        .map_err(|(status, message)| (status, message.to_string()))?;
    let requested = request.session_id.trim();
    if requested.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "session_id is required".to_string(),
        ));
    }
    let (provider_id, previous) = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .map(|tab| (tab.agent_type.clone(), tab.agent_session_id.clone()))
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Tab not found".to_string()))?;
    let provider_id = provider_id.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            format!(
                "Tab {tab_id} has no provider; found session {}, requested session '{}'",
                previous.as_deref().unwrap_or("-"),
                requested,
            ),
        )
    })?;
    let claim = match rebind_session_core(
        &state.svc.mux,
        state.svc.agent_registry.as_ref(),
        &tab_id,
        &provider_id,
        requested,
    ) {
        Ok(claim) => claim,
        Err((status, message, _)) => {
            let reason = format!(
                "Tab {tab_id} remains bound to {}; requested session '{}' was refused: {message}",
                previous.as_deref().unwrap_or("-"),
                requested,
            );
            state
                .svc
                .deferred_session_bindings
                .refuse(&tab_id, requested, previous.as_deref(), &reason);
            state.svc.emit_session_binding_state(&tab_id);
            return Err((status, reason));
        }
    };
    let response = finish_session_binding(
        &state,
        &tab_id,
        &provider_id,
        requested,
        None,
        "cli-rebind",
        &serde_json::Value::Null,
        claim,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "tab_id": tab_id,
        "found_session_id": previous,
        "requested_session_id": requested,
        "changed": response.0.get("changed").and_then(|value| value.as_bool()).unwrap_or(false),
    })))
}

pub(crate) fn rebind_session_core(
    mux: &crate::multiplexer::commands::MuxState,
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
) -> Result<SessionStartClaim, SessionBindError> {
    bind_reported_session(mux, registry, tab_id, provider_id, session_id, None)
}
