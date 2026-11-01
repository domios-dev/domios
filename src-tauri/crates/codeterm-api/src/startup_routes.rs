use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use codeterm_service_core::{config::ConfigState, startup::desk::StartupDesk};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
struct Desktop {
    desk: StartupDesk,
    config: ConfigState,
    events: codeterm_events::EventBroadcasterState,
    storage_error: Option<String>,
    account: Option<codeterm_service_core::service::account::startup::StartupAccount>,
}

pub fn serves(path: &str) -> bool {
    !path.starts_with("/api/")
        || matches!(
            path,
            "/api/mux/state"
                | "/api/config"
                | "/api/events/stream"
                | "/api/account/status"
                | "/api/account/plan"
        )
        || (path.starts_with("/api/workspaces/") && path.ends_with("/focus"))
        || (path.starts_with("/api/tabs/") && path.ends_with("/focus"))
        || path.starts_with("/api/frontend/")
}

pub fn router(
    states: &codeterm_service_core::startup::AppStates,
    dist: Option<PathBuf>,
    bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
    account: Option<codeterm_service_core::service::account::startup::StartupAccount>,
) -> Router {
    let desktop = Desktop {
        desk: states.startup_desk.clone(),
        config: states.config.clone(),
        events: states.event_broadcaster.clone(),
        storage_error: states.storage.unavailable_reason().map(str::to_owned),
        account,
    };
    let router = desktop_routes(desktop.clone(), dist);
    if let Some(bundles) = bundles {
        let token = states.config.lock().api.token.clone();
        router.merge(
            crate::frontend_bundle_routes::routes(
                crate::frontend_bundle_routes::FrontendRouteState {
                    bundles,
                    is_owner: Arc::new(move |headers| bearer_matches(headers, token.as_deref())),
                },
            )
            .layer(middleware::from_fn_with_state(desktop, authorize)),
        )
    } else {
        router
    }
}

fn desktop_routes(desktop: Desktop, dist: Option<PathBuf>) -> Router {
    let router = desktop_api(desktop).merge(crate::routes::retired_web_routes());
    if let Some(dist) = dist {
        router
            .merge(crate::routes::bundled_frontend_routes(&dist))
            .fallback_service(tower_http::services::ServeDir::new(&dist).fallback(
                tower_http::services::ServeFile::new(dist.join("index.html")),
            ))
    } else {
        router
    }
}

fn desktop_api(desktop: Desktop) -> Router {
    Router::new()
        .route("/api/mux/state", get(snapshot))
        .route("/api/config", get(config))
        .route("/api/events/stream", get(events))
        .route("/api/account/status", get(account_status))
        .route("/api/account/plan", get(account_plan))
        .route("/api/workspaces/{workspace}/focus", post(focus_workspace))
        .route(
            "/api/workspaces/{workspace}/tabs/{tab}/focus",
            post(focus_tab),
        )
        .route("/api/tabs/{tab}/focus", post(focus_pane))
        .route(
            "/api/{*path}",
            any(|| async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "ok": false, "error": {"code": "starting", "subsystem": "service", "request_executed": false}
                    })),
                )
            }),
        )
        .layer(middleware::from_fn_with_state(desktop.clone(), authorize))
        .with_state(desktop)
}

fn bearer_matches(headers: &HeaderMap, token: Option<&str>) -> bool {
    token
        .filter(|token| !token.is_empty())
        .is_some_and(|token| {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .is_some_and(|value| {
                    value.as_bytes().strip_prefix(b"Bearer ") == Some(token.as_bytes())
                })
        })
}

async fn authorize(
    State(state): State<Desktop>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    if request.uri().path().starts_with("/api/")
        && !bearer_matches(request.headers(), state.config.lock().api.token.as_deref())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

async fn snapshot(State(state): State<Desktop>) -> Response {
    if let Some(error) = state.storage_error {
        return (StatusCode::SERVICE_UNAVAILABLE, error).into_response();
    }
    tokio::task::spawn_blocking(move || Json(state.desk.snapshot()).into_response())
        .await
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn config(State(state): State<Desktop>) -> Response {
    match serde_json::to_value(&*state.config.lock()) {
        Ok(value) => Json(crate::config_routes::redact_rest_config(value)).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn events(
    State(state): State<Desktop>,
    Query(query): Query<crate::web_routes::EventsStreamQuery>,
) -> Response {
    crate::web_routes::events_sse(state.events, query, &crate::stream_census::STREAMS)
        .into_response()
}

fn starting(area: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "ok": false, "error": {"code": "starting", "subsystem": area}
        })),
    )
        .into_response()
}

async fn account_status(State(state): State<Desktop>) -> Response {
    match state.account.filter(|account| account.is_valid()) {
        Some(account) => Json(account.status()).into_response(),
        None => starting("account"),
    }
}

async fn account_plan(State(state): State<Desktop>) -> Response {
    match state.account.and_then(|account| account.plan()) {
        Some(plan) => Json(plan).into_response(),
        None => starting("billing"),
    }
}

fn focused(result: Result<(), String>) -> Response {
    match result {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(error) => (StatusCode::NOT_FOUND, error).into_response(),
    }
}

async fn focus_workspace(State(state): State<Desktop>, Path(workspace): Path<String>) -> Response {
    focused(state.desk.focus_workspace(&workspace))
}

async fn focus_tab(
    State(state): State<Desktop>,
    Path((workspace, tab)): Path<(String, String)>,
) -> Response {
    focused(state.desk.focus_tab(&workspace, &tab))
}

async fn focus_pane(State(state): State<Desktop>, Path(tab): Path<String>) -> Response {
    focused(state.desk.focus_pane(&tab))
}

#[cfg(test)]
mod tests;
