use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::routes::AppState;
use codeterm_service_core::plan::OrchestratorRef;

use super::{emit_plan_changed, epoch_ms, plan_err, text_err, PlanRouteResult};

#[derive(Deserialize)]
pub struct MilestoneAddBody {
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    add_kind: bool,
    #[serde(default)]
    from: Option<OrchestratorRef>,
}

pub async fn milestone_add(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<MilestoneAddBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let from = crate::auth::actor_ref(&state, &headers, body.from)
            .map_err(|(status, message)| text_err(status, message))?;
        let mid = state
            .svc
            .plan_store
            .add_milestone(
                &id,
                &tid,
                &body.kind,
                &body.text,
                from,
                body.add_kind,
                epoch_ms(),
            )
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "milestone_id": mid })))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct MilestoneUpdateBody {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

pub async fn milestone_update(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, tid, ms_id)): Path<(String, String, String)>,
    Json(body): Json<MilestoneUpdateBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .update_milestone(
                &id,
                &tid,
                &ms_id,
                body.kind.as_deref(),
                body.text.as_deref(),
                epoch_ms(),
            )
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

pub async fn milestone_rm(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, tid, ms_id)): Path<(String, String, String)>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        state
            .svc
            .plan_store
            .rm_milestone(&id, &tid, &ms_id, epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}
