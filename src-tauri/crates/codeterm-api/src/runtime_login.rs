use super::routes::AppState;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use codeterm_service_core::service::agent_runtime::login::{operate, LoginRequest};

pub(super) async fn tab_usage(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::usage_for_tab(&state.svc, &tab)
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn background_processes(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    axum::extract::Query(request): axum::extract::Query<
        codeterm_service_core::service::agent_runtime::background::BackgroundProcessAuthority,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::background::BackgroundProcessSnapshot>,
    (StatusCode, String),
> {
    codeterm_service_core::service::agent_runtime::background::list_for_tab(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn stop_background_process(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::background::StopBackgroundProcessRequest,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::background::BackgroundProcessSnapshot>,
    (StatusCode, String),
> {
    codeterm_service_core::service::agent_runtime::background::stop_for_tab(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn child_history(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path((tab, thread)): Path<(String, String)>,
    axum::extract::Query(request): axum::extract::Query<
        codeterm_service_core::service::agent_runtime::children::ChildHistoryRequest,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::children::ChildHistoryPage>,
    (StatusCode, String),
> {
    codeterm_service_core::service::agent_runtime::children::history(
        &state.svc, &tab, &thread, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn show_login(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
) -> Result<(HeaderMap, Json<serde_json::Value>), (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::login::show(&state.svc, &tab)
        .map(login_response)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<LoginRequest>,
) -> Result<(HeaderMap, Json<serde_json::Value>), (StatusCode, String)> {
    if request.api_key.is_some() && !super::auth::is_local_caller(&state, &headers) {
        return Err((
            StatusCode::FORBIDDEN,
            "API keys must be entered on the provider's own machine".into(),
        ));
    }
    if request.code.is_some() && !super::auth::is_local_caller(&state, &headers) {
        return Err((
            StatusCode::FORBIDDEN,
            "Sign-in codes must be entered on the provider's own machine".into(),
        ));
    }
    operate(&state.svc, &tab, request)
        .await
        .map(login_response)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn reset_credits(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::reset_credits::ResetCreditsRequest,
    >,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::reset_credits::operate(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn goal(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<codeterm_service_core::service::agent_runtime::goals::GoalRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::goals::operate(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn thread_history(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::thread_history::ThreadHistoryRequest,
    >,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::thread_history::operate(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

/// Permission choices belong to the owner and the owner's paired machines, never to an agent:
/// one answering or widening its own questions would defeat "Ask each time".
pub(crate) fn require_permission_authority(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, String)> {
    match crate::auth::RequestActor::authenticate(state, headers)? {
        crate::auth::RequestActor::Owner | crate::auth::RequestActor::Peer => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            "Only the owner changes a tab's permissions".to_string(),
        )),
    }
}

async fn controls(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::declared_controls::Selection,
    >,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use codeterm_service_core::service::agent_runtime::declared_controls::{
        select, ControlError, RUNTIME_BUSY_MESSAGE, RUNTIME_BUSY_REASON,
    };
    if state
        .svc
        .runtime_control_requires_owner(&tab, request.mode, &request.id)
    {
        require_permission_authority(&state, &headers)?;
    }
    select(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|error| match error {
            ControlError::Busy => (
                StatusCode::CONFLICT,
                serde_json::json!({"ok":false,"reason":RUNTIME_BUSY_REASON,"error":RUNTIME_BUSY_MESSAGE})
                    .to_string(),
            ),
            ControlError::Refused(reason) => (StatusCode::CONFLICT, reason),
        })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostApprovalsRequest {
    allow_all: bool,
}

async fn host_approvals(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw_tab): Path<String>,
    Json(request): Json<HostApprovalsRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_permission_authority(&state, &headers)?;
    let tab = crate::routes::resolve_tab_async(&state, &raw_tab)
        .await
        .map_err(|(status, message)| (status, message.to_string()))?;
    let svc = state.svc.clone();
    let target = tab.clone();
    let allow_all =
        tokio::task::spawn_blocking(move || svc.set_tab_host_approvals(&target, request.allow_all))
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "host approvals task join failed".to_string(),
                )
            })?
            .map_err(|reason| (StatusCode::CONFLICT, reason))?;
    Ok(Json(
        serde_json::json!({"tab_id": tab, "allow_all": allow_all}),
    ))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tabs/{id}/agent/runtime/controls", post(controls))
        .route(
            "/api/tabs/{id}/agent/runtime/host-approvals",
            post(host_approvals),
        )
        .route("/api/tabs/{id}/agent/runtime/goal", post(goal))
        .route("/api/tabs/{id}/agent/runtime/usage", get(tab_usage))
        .route(
            "/api/tabs/{id}/agent/runtime/thread-history",
            post(thread_history),
        )
        .route(
            "/api/tabs/{id}/agent/runtime/reset-credits",
            post(reset_credits),
        )
        .route(
            "/api/tabs/{id}/agent/runtime/login",
            get(show_login).post(login),
        )
        .route(
            "/api/tabs/{id}/agent/runtime/children/{thread}/history",
            get(child_history),
        )
        .route(
            "/api/tabs/{id}/agent/runtime/background-processes",
            get(background_processes).post(stop_background_process),
        )
}

#[cfg(test)]
mod tests;

fn login_response(value: serde_json::Value) -> (HeaderMap, Json<serde_json::Value>) {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    (headers, Json(value))
}
