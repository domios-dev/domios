use axum::extract::State as AxumState;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

use crate::routes::AppState;
use codeterm_service_core::service::general_agent::FreshGeneralSessionRequest;
use codeterm_service_core::service::transport::AtomicSpawn;

pub async fn spawn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<FreshGeneralSessionRequest>,
) -> Result<Json<AtomicSpawn<serde_json::Value>>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let from_mesh = matches!(
            crate::auth::authenticated_origin(&state, &headers),
            Ok(crate::auth::AuthenticatedOrigin::Mesh)
        );
        state
            .svc
            .spawn_fresh_general_session_for_origin(&state.config, request, !from_mesh)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
