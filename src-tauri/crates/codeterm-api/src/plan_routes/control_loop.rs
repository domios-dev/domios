use super::{text_err, PlanRouteResult};
use crate::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/plans/{id}/state", axum::routing::get(super::state))
        .route(
            "/api/plans/{id}/control-loop",
            axum::routing::get(show).post(set),
        )
        .route("/api/plans/{id}/control-loop/run", axum::routing::post(run))
}

pub async fn run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(status, message)| text_err(status, message))?;
    let caller = crate::tab_routes::authenticated_plan_caller(&headers);
    super::authorize_plan_orchestrator_http(&state.svc, &id, caller.clone()).await?;
    let result = tokio::task::spawn_blocking(move || {
        state
            .svc
            .control_loop_run(&id, &caller, state.svc.curator_now()?)
    })
    .await
    .map_err(|error| text_err(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    result
        .map(|value| Json(serde_json::json!(value)))
        .map_err(|error| text_err(StatusCode::BAD_REQUEST, error))
}

pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let result = tokio::task::spawn_blocking(move || state.svc.control_loop_show(&id))
        .await
        .map_err(|e| text_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    result
        .map(|v| Json(serde_json::json!(v)))
        .map_err(|e| text_err(StatusCode::BAD_REQUEST, e))
}

pub async fn set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<codeterm_plan::service_plan::curator::ControlLoopUpdate>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let caller = crate::tab_routes::authenticated_plan_caller(&headers);
    super::authorize_plan_orchestrator_http(&state.svc, &id, caller.clone()).await?;
    let result = tokio::task::spawn_blocking(move || {
        state
            .svc
            .control_loop_set(&id, &caller, body, state.svc.curator_now()?)
    })
    .await
    .map_err(|e| text_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    result
        .map(|v| Json(serde_json::json!(v)))
        .map_err(|e| text_err(StatusCode::BAD_REQUEST, e))
}
