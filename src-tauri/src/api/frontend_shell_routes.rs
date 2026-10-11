use super::{auth::is_local_caller, routes::AppState};
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
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
        .route("/api/listener/restart", post(listener_restart))
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
) -> Result<Json<Value>, ApiError> {
    owner(&state, &headers)?;
    let status = state.rest_api.status();
    let mut value = serde_json::to_value(&status)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    value["bindError"] =
        if status.diagnostic == Some(crate::startup::RestApiDiagnostic::BindOwnershipFailed) {
            json!(status.message)
        } else {
            Value::Null
        };
    Ok(Json(value))
}

async fn listener_restart(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    owner(&state, &headers)?;
    let accepted = state
        .rest_api
        .prepare_http_restart()
        .await
        .map_err(|error| (StatusCode::CONFLICT, error))?;
    let status = json!({ "generation": accepted.generation, "phase": "starting", "diagnostic": null, "message": null });
    let bytes = serde_json::to_vec(&status)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok((
        StatusCode::ACCEPTED,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        Body::from_stream(restart_response(
            Bytes::from(bytes),
            accepted.response_complete,
        )),
    )
        .into_response())
}

fn restart_response(
    bytes: Bytes,
    completed: tokio::sync::oneshot::Sender<()>,
) -> impl futures_core::Stream<Item = Result<Bytes, std::convert::Infallible>> {
    async_stream::stream! {
        yield Ok(bytes);
        let _ = completed.send(());
    }
}

#[cfg(test)]
mod tests;
