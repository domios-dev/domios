use axum::extract::State as AxumState;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

use crate::api::routes::{check_auth, AppState};
use crate::service::general_agent::FreshGeneralSessionRequest;
use crate::service::transport::AtomicSpawn;

pub async fn spawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<FreshGeneralSessionRequest>,
) -> Result<Json<AtomicSpawn>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
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
