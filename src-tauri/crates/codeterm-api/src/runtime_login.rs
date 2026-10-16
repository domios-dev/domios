use super::routes::{AppState, check_auth};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use codeterm_service_core::service::agent_runtime::login::{LoginRequest, operate};

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

pub(super) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, reason)| (status, reason.to_string()))?;
    if request.api_key.is_some() && !super::auth::is_local_caller(&state, &headers) {
        return Err((
            StatusCode::FORBIDDEN,
            "API keys must be entered on the provider's own machine".into(),
        ));
    }
    operate(&state.svc, &tab, request)
        .await
        .map(Json)
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
