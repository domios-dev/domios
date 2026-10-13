use crate::api::{auth::is_local_caller, routes::AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use codeterm_frontend_bundle::FrontendBundles;
use serde::Deserialize;
use std::sync::Arc;

type ApiError = (StatusCode, String);

#[derive(Clone)]
pub struct FrontendRouteState {
    pub bundles: Arc<FrontendBundles>,
    pub is_owner: Arc<dyn Fn(&HeaderMap) -> bool + Send + Sync>,
}

pub fn router(app: AppState, bundles: Arc<FrontendBundles>) -> Router<AppState> {
    let service = app.svc.clone();
    let weak_bundles = Arc::downgrade(&bundles);
    bundles.bind_installation_observer(Arc::new(move || {
        service.record_product_event(codeterm_analytics::Event::UpdateInstalled {
            kind: codeterm_analytics::UpdateKind::Frontend,
        });
        if let Some(bundles) = weak_bundles.upgrade() {
            if let Some(version) = bundles.status()["active"]["version"].as_str() {
                crate::update::discovery::frontend_installed(&service.app_handle, version);
            }
        }
    }));
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
    let deadline_ms = now.saturating_add(30_000);
    let deadline_bundles = bundles.clone();
    let result =
        work(move || bundles.activate(&body.release_id, body.clients, deadline_ms)).await?;
    tokio::spawn(async move {
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            std::future::pending::<()>(),
        )
        .await;
        let _ = work(move || deadline_bundles.failure_deadline(deadline_ms)).await;
    });
    Ok(Json(result))
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
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "webmanifest" => "application/manifest+json",
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
        codeterm_frontend_bundle::OBSERVER_SCRIPT,
    )
        .into_response()
}

#[cfg(test)]
mod tests;
