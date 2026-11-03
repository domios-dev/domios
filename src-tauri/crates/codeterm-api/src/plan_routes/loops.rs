use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::routes::AppState;
use codeterm_service_core::service::plan::PlanError;

use super::{emit_plan_changed, epoch_ms, plan_err, PlanRouteResult};

#[derive(Deserialize)]
pub struct LoopCreateBody {
    target: String,
    budget: u32,
    #[serde(default)]
    stop_when: Option<String>,
}

pub async fn loop_create(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LoopCreateBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let lid = state
            .svc
            .plan_store
            .create_loop(
                &id,
                &body.target,
                body.budget,
                body.stop_when.as_deref(),
                epoch_ms(),
            )
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "loop_id": lid })))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct LoopSetBody {
    #[serde(default)]
    budget: Option<u32>,
    #[serde(default)]
    stop_when: Option<String>,
}

pub async fn loop_set(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((_id, lid)): Path<(String, String)>,
    Json(body): Json<LoopSetBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .set_loop(
                &lid,
                body.budget,
                Some(body.stop_when.as_deref()),
                epoch_ms(),
            )
            .map_err(plan_err)?;
        if let Some(plan_id) = state.svc.plan_store.list_index().iter().find_map(|e| {
            state
                .svc
                .plan_store
                .get(&e.plan_id)
                .filter(|p| p.loops.iter().any(|l| l.id == lid))
                .map(|p| p.id)
        }) {
            emit_plan_changed(&state, &plan_id);
        }
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

pub async fn loop_status(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((_id, lid)): Path<(String, String)>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let spec = state
            .svc
            .plan_store
            .loop_status(&lid)
            .ok_or_else(|| plan_err(PlanError::NotFound))?;
        Ok(Json(serde_json::to_value(spec).unwrap_or_default()))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct LoopRecordBody {
    note: String,
    #[serde(default)]
    stop: bool,
    #[serde(default)]
    task_id: Option<String>,
}

pub async fn loop_record(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, lid)): Path<(String, String)>,
    Json(body): Json<LoopRecordBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let result = state
            .svc
            .plan_store
            .record_loop(
                &lid,
                &body.note,
                body.stop,
                body.task_id.as_deref(),
                epoch_ms(),
            )
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::to_value(result).unwrap_or_default()))
    })
    .await
}

pub async fn loop_stop(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((_id, lid)): Path<(String, String)>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .stop_loop(&lid, epoch_ms())
            .map_err(plan_err)?;
        if let Some(plan_id) = state.svc.plan_store.list_index().iter().find_map(|e| {
            state
                .svc
                .plan_store
                .get(&e.plan_id)
                .filter(|p| p.loops.iter().any(|l| l.id == lid))
                .map(|p| p.id)
        }) {
            emit_plan_changed(&state, &plan_id);
        }
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}
