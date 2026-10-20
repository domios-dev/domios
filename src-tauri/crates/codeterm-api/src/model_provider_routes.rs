use super::routes::AppState;
use axum::{
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::service::model_providers::{ModelProviderError, ModelProviderRequest};
use std::net::SocketAddr;

#[derive(serde::Deserialize)]
pub struct ModelProviderScope {
    provider: Option<String>,
}

pub async fn manage(
    State(state): State<AppState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(scope): Query<ModelProviderScope>,
    Json(request): Json<ModelProviderRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let token = state.config.lock().api.token.clone();
    if !super::cli_rpc_routes::local_caller(
        remote,
        token.as_deref(),
        headers.get("authorization").and_then(|v| v.to_str().ok()),
    ) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"kind":"local_only"})),
        ));
    }
    let result = tokio::task::spawn_blocking(move || {
        state
            .svc
            .model_provider_command(scope.provider.as_deref(), request)
    })
    .await
    .map_err(|_| ModelProviderError::Config)
    .and_then(|result| result);
    result.map(Json).map_err(|kind| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"kind":kind})),
        )
    })
}
