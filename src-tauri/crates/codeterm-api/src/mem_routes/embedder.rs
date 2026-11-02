use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::mem_ops::embedder::{self, Refusal, UseParams};

type Reply = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn failure(status: StatusCode, message: String, hints: Vec<String>) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(serde_json::json!({ "ok": false, "error": { "message": message, "hints": hints } })),
    )
}

fn refused(refusal: Refusal) -> (StatusCode, Json<Value>) {
    failure(StatusCode::CONFLICT, refusal.message, refusal.hints)
}

async fn blocking(
    state: AppState,
    headers: HeaderMap,
    work: impl FnOnce(codeterm_service_core::service::ServiceState) -> Result<Value, Refusal>
        + Send
        + 'static,
) -> Reply {
    check_auth(&state, &headers)
        .map_err(|(status, error)| failure(status, error.to_string(), Vec::new()))?;
    let svc = state.svc.clone();
    tokio::task::spawn_blocking(move || work(svc))
        .await
        .map_err(|error| {
            failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("join: {error}"),
                Vec::new(),
            )
        })?
        .map(Json)
        .map_err(refused)
}

fn json(value: impl serde::Serialize) -> Result<Value, Refusal> {
    serde_json::to_value(value).map_err(|error| Refusal {
        message: error.to_string(),
        hints: Vec::new(),
    })
}

pub async fn mem_embedder_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Reply {
    blocking(state, headers, |svc| json(svc.mem.embedder_status())).await
}

pub async fn mem_embedder_providers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Reply {
    blocking(state, headers, |svc| json(embedder::list(&svc))).await
}

pub async fn mem_embedder_preview(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(params): Query<UseParams>,
) -> Reply {
    blocking(state, headers, move |svc| {
        json(embedder::preview(&svc, &params)?)
    })
    .await
}

pub async fn mem_embedder_use(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(params): Json<UseParams>,
) -> Reply {
    blocking(state, headers, move |svc| {
        json(embedder::use_provider(&svc, &params)?)
    })
    .await
}
