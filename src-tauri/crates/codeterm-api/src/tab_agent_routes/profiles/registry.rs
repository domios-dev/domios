use crate::routes::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use codeterm_service_core::service::agent_runtime::{acp_profiles, launch_profile_setup::SetupJob};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub(crate) struct Search {
    #[serde(default)]
    q: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Install {
    id: String,
}

pub(crate) async fn search(
    Query(query): Query<Search>,
) -> Result<Json<Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || acp_profiles::registry_search(&query.q))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Registry search worker unavailable".into(),
            )
        })?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

pub(crate) async fn install(
    State(state): State<AppState>,
    Json(request): Json<Install>,
) -> Result<(StatusCode, Json<SetupJob>), (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        acp_profiles::begin_registry_install(&state.svc, request.id)
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Registry install worker unavailable".into(),
        )
    })?
    .map(|job| (StatusCode::ACCEPTED, Json(job)))
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

pub(crate) async fn remove(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || acp_profiles::remove(&state.svc, &name))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Profile removal worker unavailable".into(),
            )
        })?
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

#[cfg(test)]
mod tests;
