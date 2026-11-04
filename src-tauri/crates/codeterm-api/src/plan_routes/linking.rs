use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::routes::AppState;

use super::{emit_plan_changed, epoch_ms, plan_err, PlanRouteResult};

#[derive(Deserialize)]
pub struct LinkBody {
    to: String,
}

pub async fn link(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LinkBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .link_plan(&id, &body.to, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

pub async fn unlink(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, to)): Path<(String, String)>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .unlink_plan(&id, &to, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}
