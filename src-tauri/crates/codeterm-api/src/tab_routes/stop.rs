use crate::routes::AppState;
use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

pub async fn stop_tab(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
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
