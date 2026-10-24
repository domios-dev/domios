use super::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use codeterm_service_core::service::agent_runtime::login::{operate, LoginRequest};

pub(super) async fn tab_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    codeterm_service_core::service::agent_runtime::usage_for_tab(&state.svc, &tab)
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn background_processes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    axum::extract::Query(request): axum::extract::Query<
        codeterm_service_core::service::agent_runtime::background::BackgroundProcessAuthority,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::background::BackgroundProcessSnapshot>,
    (StatusCode, String),
> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    codeterm_service_core::service::agent_runtime::background::list_for_tab(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn stop_background_process(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::background::StopBackgroundProcessRequest,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::background::BackgroundProcessSnapshot>,
    (StatusCode, String),
> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    codeterm_service_core::service::agent_runtime::background::stop_for_tab(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn child_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((tab, thread)): Path<(String, String)>,
    axum::extract::Query(request): axum::extract::Query<
        codeterm_service_core::service::agent_runtime::children::ChildHistoryRequest,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::children::ChildHistoryPage>,
    (StatusCode, String),
> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    codeterm_service_core::service::agent_runtime::children::history(
        &state.svc, &tab, &thread, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn show_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
) -> Result<(HeaderMap, Json<serde_json::Value>), (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
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
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    if request.api_key.is_some() && !super::auth::is_local_caller(&state, &headers) {
        return Err((
            StatusCode::FORBIDDEN,
            "API keys must be entered on the provider's own machine".into(),
        ));
    }
    operate(&state.svc, &tab, request)
        .await
        .map(login_response)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn reset_credits(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::reset_credits::ResetCreditsRequest,
    >,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    codeterm_service_core::service::agent_runtime::reset_credits::operate(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn goal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<codeterm_service_core::service::agent_runtime::goals::GoalRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    codeterm_service_core::service::agent_runtime::goals::operate(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn thread_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::thread_history::ThreadHistoryRequest,
    >,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    codeterm_service_core::service::agent_runtime::thread_history::operate(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
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
