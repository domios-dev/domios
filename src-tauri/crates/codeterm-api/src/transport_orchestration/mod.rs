use super::{auth::is_local_caller, routes::AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use codeterm_bridge::bridge::threads::{authorize_open, ThreadCaller, ThreadError};
use serde::Deserialize;
use serde_json::{json, Value};

pub mod authority;
mod node_files;
mod team;
#[cfg(test)]
mod tests;

pub type RouteError = (StatusCode, String);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{team_id}/rename", post(team::rename))
        .route("/api/teams/{team_id}/close", post(team::close))
        .route("/api/teams/{team_id}/members", post(team::member))
        .route("/api/orchestration/new", post(team::pair))
        .route(
            "/api/tabs/{id}/make-orchestrator",
            post(team::make_orchestrator),
        )
        .route("/api/tabs/{id}/make-planner", post(team::make_planner))
        .route("/api/automation/node-files", post(node_files::node_files))
        .route(
            "/api/tabs/{id}/bridge/thread",
            get(thread_state).post(thread_open).delete(thread_close),
        )
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadAction {
    selected_tab_id: String,
    #[serde(flatten)]
    claim: authority::CallerClaim,
}

fn bridge_target(state: &AppState, headers: &HeaderMap, id: &str) -> Result<String, RouteError> {
    authority::require_owner(is_local_caller(state, headers))?;
    state
        .svc
        .mux
        .read()
        .resolve_tab_id(id)
        .ok_or((StatusCode::NOT_FOUND, "Target tab not found".into()))
}

fn bridge_action(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    body: &ThreadAction,
) -> Result<(String, Option<String>), RouteError> {
    let target = bridge_target(state, headers, id)?;
    let selected = state.svc.mux.read().resolve_tab_id(&body.selected_tab_id);
    if selected.as_deref() != Some(target.as_str()) {
        return Err((
            StatusCode::FORBIDDEN,
            "Explicit selected tab must match target".into(),
        ));
    }
    let caller = authority::validate_claim(state, headers, &body.claim)?;
    let thread_caller = caller
        .as_deref()
        .map_or(ThreadCaller::Owner, |tab_id| ThreadCaller::Tab {
            tab_id,
            is_general: state.svc.bridge_is_general_tab(tab_id),
        });
    authorize_open(thread_caller, &target).map_err(thread_error)?;
    Ok((target, caller))
}

fn thread_error(error: ThreadError) -> RouteError {
    let status = match &error {
        ThreadError::ForeignTarget { .. } => StatusCode::FORBIDDEN,
        ThreadError::UnknownTab(_) => StatusCode::NOT_FOUND,
        ThreadError::CreationFailed(_) => StatusCode::BAD_GATEWAY,
        ThreadError::BridgeInactive => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::CONFLICT,
    };
    (status, error.to_string())
}

async fn thread_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, RouteError> {
    let owner = state.clone();
    let target = tokio::task::spawn_blocking(move || {
        let state = owner;
        let target = bridge_target(&state, &headers, &id)?;
        let caller =
            authority::validate_claim(&state, &headers, &authority::CallerClaim::default())?;
        let thread_caller =
            caller
                .as_deref()
                .map_or(ThreadCaller::Owner, |tab_id| ThreadCaller::Tab {
                    tab_id,
                    is_general: state.svc.bridge_is_general_tab(tab_id),
                });
        authorize_open(thread_caller, &target).map_err(thread_error)?;
        Ok::<_, RouteError>(target)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    serde_json::to_value(state.svc.bridge_thread_menu_state(&target).await)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn thread_open(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ThreadAction>,
) -> Result<Json<Value>, RouteError> {
    let owner = state.clone();
    let (target, caller) =
        tokio::task::spawn_blocking(move || bridge_action(&owner, &headers, &id, &body))
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    let opened = state
        .svc
        .open_bridge_thread(caller.as_deref(), &target, None)
        .await
        .map_err(thread_error)?;
    serde_json::to_value(opened)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn thread_close(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ThreadAction>,
) -> Result<Json<Value>, RouteError> {
    let owner = state.clone();
    let (target, _) =
        tokio::task::spawn_blocking(move || bridge_action(&owner, &headers, &id, &body))
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    Ok(Json(
        json!({ "outcomes": state.svc.close_bridge_thread(&target).await }),
    ))
}
