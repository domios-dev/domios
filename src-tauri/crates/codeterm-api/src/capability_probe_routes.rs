use super::routes::AppState;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
pub async fn refresh(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
) -> Response {
    let tab = match crate::routes::resolve_tab_async(&state, &tab).await {
        Ok(tab) => tab,
        Err((status, message)) => return (status, message).into_response(),
    };
    match tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::capability_probe_refresh(&state.svc, &tab, true)
    })
    .await
    {
        Ok(result) => Json(result).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}
