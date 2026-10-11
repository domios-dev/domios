use super::{auth::is_local_caller, routes::AppState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

type ApiError = (StatusCode, String);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/frontend/bootstrap", get(bootstrap))
        .route("/api/frontend/ready", post(ready))
        .route("/api/desktop/recovery", get(recovery))
        .route("/api/listener/status", get(listener_status))
}

fn owner(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    if is_local_caller(state, headers) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Desktop lifecycle requires owner authority".into(),
        ))
    }
}

async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    owner(&state, &headers)?;
    Ok(Json(json!({
        "generation": crate::startup::frontend_readiness_generation(),
        "apiVersion": crate::frontend_bundle_api_version::API_VERSION,
        "bundled": {
            "index": "/bundled/index.html",
            "minApiVersion": crate::frontend_bundle_api_version::BUNDLED_MIN_API_VERSION,
            "maxApiVersion": crate::frontend_bundle_api_version::BUNDLED_MAX_API_VERSION,
        },
        "listener": state.rest_api.status(),
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    generation: u64,
}

async fn ready(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Ready>,
) -> Result<Json<bool>, ApiError> {
    owner(&state, &headers)?;
    Ok(Json(crate::startup::mark_frontend_ready(body.generation)))
}

async fn recovery(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::crash::lifecycle::RecoveryState>, ApiError> {
    owner(&state, &headers)?;
    tokio::task::spawn_blocking(crate::crash::lifecycle::recovery_state)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn listener_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::startup::RestApiStatus>, ApiError> {
    owner(&state, &headers)?;
    Ok(Json(state.rest_api.status()))
}

#[cfg(test)]
mod tests;
