use crate::routes::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use codeterm_service_core::service::provider_accounts;

pub(super) async fn list(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<provider_accounts::roster::Snapshot>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || provider_accounts::roster::snapshot(&state.svc, &provider))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "The account list is unavailable".into(),
            )
        })?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

pub(super) async fn update(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(request): Json<provider_accounts::Request>,
) -> Result<Json<provider_accounts::roster::Snapshot>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || provider_accounts::update(&state.svc, &provider, request))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "The account change could not finish".into(),
            )
        })?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
