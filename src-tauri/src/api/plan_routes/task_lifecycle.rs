use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::plan::AgentRef;
use crate::service::ServiceState;

use super::tasks::{verify_task_mutation_owner, TaskMutationKind};
use super::{emit_plan_changed, epoch_ms, plan_err, text_err, PlanRouteResult};

#[derive(Deserialize)]
pub struct TaskPauseBody {
    reason: String,
    #[serde(default)]
    from: Option<AgentRef>,
}

/// REST twin of `plan.task.pause`: same owner-epoch gate, same store transition.
pub async fn task_pause(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
    Json(body): Json<TaskPauseBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let response = task_pause_core(&state.svc, &headers, &id, &tid, body)?;
    emit_plan_changed(&state, &id);
    Ok(response)
}

/// REST twin of `plan.task.resume`: same owner-epoch gate, same store transition.
pub async fn task_resume(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, tid)): Path<(String, String)>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let response = task_resume_core(&state.svc, &headers, &id, &tid)?;
    emit_plan_changed(&state, &id);
    Ok(response)
}

fn task_pause_core(
    svc: &ServiceState,
    headers: &HeaderMap,
    id: &str,
    tid: &str,
    body: TaskPauseBody,
) -> PlanRouteResult {
    verify_task_mutation_owner(svc, headers, id, tid, TaskMutationKind::Recovery)?;
    let from = body.from.unwrap_or_else(|| caller_ref(headers));
    let milestone_id = svc
        .plan_store
        .pause_task(id, tid, &body.reason, from, epoch_ms())
        .map_err(plan_err)?;
    svc.reconcile_time_reminder_eligibility();
    Ok(lifecycle_response(id, tid, milestone_id))
}

fn task_resume_core(
    svc: &ServiceState,
    headers: &HeaderMap,
    id: &str,
    tid: &str,
) -> PlanRouteResult {
    verify_task_mutation_owner(svc, headers, id, tid, TaskMutationKind::Recovery)?;
    let milestone_id = svc
        .plan_store
        .resume_task(id, tid, caller_ref(headers), epoch_ms())
        .map_err(plan_err)?;
    svc.reconcile_time_reminder_eligibility();
    Ok(lifecycle_response(id, tid, milestone_id))
}

fn caller_ref(headers: &HeaderMap) -> AgentRef {
    let caller = crate::api::tab_routes::authenticated_plan_caller(headers);
    AgentRef::from_tab(caller.tab_id.as_deref().unwrap_or("unknown"))
}

fn lifecycle_response(id: &str, tid: &str, milestone_id: String) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "plan_id": id,
        "task_id": tid,
        "milestone_id": milestone_id,
    }))
}

#[cfg(test)]
mod tests;
