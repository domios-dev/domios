use crate::{
    api::{auth::is_local_caller, routes::AppState},
    frontend_bundle::FrontendBundles,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

type ApiError = (StatusCode, String);

#[derive(Clone)]
pub struct FrontendRouteState {
    pub bundles: Arc<FrontendBundles>,
    pub is_owner: Arc<dyn Fn(&HeaderMap) -> bool + Send + Sync>,
}

pub fn router(app: AppState, bundles: Arc<FrontendBundles>) -> Router<AppState> {
    routes(FrontendRouteState {
        bundles,
        is_owner: Arc::new(move |headers| is_local_caller(&app, headers)),
    })
    .with_state(())
}

pub fn routes(state: FrontendRouteState) -> Router {
    Router::new()
        .route("/frontend/bootstrap.js", get(observer))
        .route("/api/frontend/status", get(status))
        .route("/api/frontend/check", post(check))
        .route("/api/frontend/activate", post(activate))
        .route("/api/frontend/healthy", post(healthy))
        .route("/api/frontend/rollback", post(rollback))
        .route(
            "/api/frontend/clients/{id}/checkpoint",
            get(checkpoint).put(put_checkpoint),
        )
        .route("/frontend/{release_id}/{*path}", get(asset))
        .with_state(state)
}

fn owner(state: &FrontendRouteState, headers: &HeaderMap) -> Result<(), ApiError> {
    if (state.is_owner)(headers) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Frontend updates require owner authority".into(),
        ))
    }
}

async fn work<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::CONFLICT, e))
}

async fn status(State(route): State<FrontendRouteState>) -> Json<serde_json::Value> {
    Json(route.bundles.status())
}

async fn check(
    State(route): State<FrontendRouteState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    owner(&route, &headers)?;
    let bundles = route.bundles;
    work(move || bundles.check()).await.map(Json)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Activate {
    release_id: String,
    clients: Vec<String>,
}

async fn activate(
    State(route): State<FrontendRouteState>,
    headers: HeaderMap,
    Json(body): Json<Activate>,
) -> Result<Json<serde_json::Value>, ApiError> {
    owner(&route, &headers)?;
    let bundles = route.bundles;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .as_millis() as u64;
    work(move || bundles.activate(&body.release_id, body.clients, now.saturating_add(30_000)))
        .await
        .map(Json)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Healthy {
    release_id: String,
    generation: u64,
    client_id: String,
    token: String,
}

async fn healthy(
    State(route): State<FrontendRouteState>,
    headers: HeaderMap,
    Json(body): Json<Healthy>,
) -> Result<Json<serde_json::Value>, ApiError> {
    owner(&route, &headers)?;
    let bundles = route.bundles;
    work(move || {
        bundles.healthy(
            &body.release_id,
            body.generation,
            &body.client_id,
            &body.token,
        )
    })
    .await
    .map(Json)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rollback {
    release_id: String,
    generation: u64,
}

async fn rollback(
    State(route): State<FrontendRouteState>,
    headers: HeaderMap,
    Json(body): Json<Rollback>,
) -> Result<Json<serde_json::Value>, ApiError> {
    owner(&route, &headers)?;
    let bundles = route.bundles;
    work(move || bundles.fail(&body.release_id, body.generation))
        .await
        .map(Json)
}

async fn checkpoint(
    State(route): State<FrontendRouteState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    owner(&route, &headers)?;
    let bundles = route.bundles;
    work(move || bundles.checkpoint(&id)).await.map(Json)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    revision: u64,
    payload: serde_json::Value,
}

async fn put_checkpoint(
    State(route): State<FrontendRouteState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Checkpoint>,
) -> Result<Json<serde_json::Value>, ApiError> {
    owner(&route, &headers)?;
    let bundles = route.bundles;
    work(move || bundles.put_checkpoint(&id, body.revision, body.payload))
        .await
        .map(Json)
}

async fn asset(
    State(route): State<FrontendRouteState>,
    Path((release, path)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let mime = match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    let bundles = route.bundles;
    let is_index = path == "index.html";
    let bytes = work(move || bundles.asset(&release, &path)).await?;
    let bytes = if is_index {
        let html = String::from_utf8(bytes).map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
        let script = "<script src=\"/frontend/bootstrap.js\"></script>";
        if html.contains("<head>") {
            html.replacen("<head>", &format!("<head>{script}"), 1)
                .into_bytes()
        } else if let Some(end) = html.find('>') {
            format!("{}{script}{}", &html[..=end], &html[end + 1..]).into_bytes()
        } else {
            return Err((StatusCode::CONFLICT, "Invalid frontend index".into()));
        }
    } else {
        bytes
    };
    Ok((
        [
            ("Content-Type", mime),
            ("Cache-Control", "public, max-age=31536000, immutable"),
            ("X-Content-Type-Options", "nosniff"),
        ],
        bytes,
    )
        .into_response())
}

async fn observer() -> Response {
    (
        [
            ("Content-Type", "text/javascript; charset=utf-8"),
            ("Cache-Control", "no-store"),
            ("X-Content-Type-Options", "nosniff"),
        ],
        include_str!("../frontend_bundle/observer.js"),
    )
        .into_response()
}
