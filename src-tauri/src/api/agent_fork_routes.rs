use super::routes::{check_auth, resolve_tab, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

pub async fn fork(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
) -> Response {
    if let Err((status, message)) = check_auth(&state, &headers) {
        return (status, message).into_response();
    }
    let tab = match resolve_tab(&state, &tab) {
        Ok(tab) => tab,
        Err((status, message)) => return (status, message).into_response(),
    };
    match tokio::task::spawn_blocking(move || state.svc.agent_fork(&tab)).await {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err(error)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"ok":false,"error":error})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}
