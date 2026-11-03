use super::{text_err, PlanRouteResult};
use crate::routes::AppState;
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
    headers: crate::auth::ActorHeaders,
    Path(id): Path<String>,
) -> PlanRouteResult {
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
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> PlanRouteResult {
    let result = tokio::task::spawn_blocking(move || state.svc.control_loop_show(&id))
        .await
        .map_err(|e| text_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    result
        .map(|v| Json(serde_json::json!(v)))
        .map_err(|e| text_err(StatusCode::BAD_REQUEST, e))
}

pub async fn set(
    State(state): State<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(id): Path<String>,
    Json(body): Json<codeterm_plan::service_plan::curator::ControlLoopUpdate>,
) -> PlanRouteResult {
    if headers
        .get("X-CodeTerm-Plan-Confirmation")
        .and_then(|value| value.to_str().ok())
        != Some(id.as_str())
    {
        return Err(text_err(
            StatusCode::FORBIDDEN,
            "Team scheduling requires owner confirmation from the Team dashboard",
        ));
    }
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
