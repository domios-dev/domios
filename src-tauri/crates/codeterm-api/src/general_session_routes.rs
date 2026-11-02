use axum::extract::State as AxumState;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

use crate::routes::AppState;
use codeterm_service_core::service::general_agent::FreshGeneralSessionRequest;
use codeterm_service_core::service::transport::AtomicSpawn;

pub async fn spawn(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(request): Json<FreshGeneralSessionRequest>,
) -> Result<Json<AtomicSpawn<serde_json::Value>>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .spawn_fresh_general_session(&state.config, request)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
