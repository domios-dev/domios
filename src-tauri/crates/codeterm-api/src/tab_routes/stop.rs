use crate::routes::{check_auth, resolve_tab, AppState};
use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

pub async fn stop_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let tab_id = resolve_tab(&state, &raw_tab_id)
        .map_err(|(status, message)| (status, message.to_string()))?;
    codeterm_service_core::service::stop_execution_stop_agent_turn((*state.svc).clone(), tab_id)
        .await
        .map(Json)
        .map_err(|message| {
            (
                if message == "Tab not found or has no PTY" {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                },
                message,
            )
        })
}
