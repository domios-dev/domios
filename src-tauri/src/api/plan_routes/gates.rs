use axum::{
    extract::{Path, Query, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::plan::peer::{resolve_planner_ref, PeerResolution};

use super::{emit_plan_changed, epoch_ms, plan_err, text_err, PlanRouteResult};

fn resolve_routed_pane(state: &AppState, plan_id: &str) -> Option<String> {
    let plan = state.svc.plan_store.get(plan_id)?;
    let resolution =
        resolve_planner_ref(&plan.planner, &plan.owner, |pid| state.svc.pane_probe(pid));
    match resolution {
        PeerResolution::Planner { pane_id } => Some(pane_id),
        PeerResolution::OrchestratorFallback { .. } => plan.owner.pane_id.clone(),
        PeerResolution::Waived { .. } | PeerResolution::Unbound => None,
    }
}

#[derive(Deserialize)]
pub(crate) struct GateAskBody {
    question: String,
    #[serde(default)]
    context: String,
    #[serde(default)]
    blocking_task_ids: Vec<String>,
    #[serde(default)]
    asked_by_pane_id: Option<String>,
}

pub async fn gate_ask(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<GateAskBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let routed_to = resolve_routed_pane(&state, &id);
    let gate = state
        .svc
        .plan_store
        .ask_gate(
            &id,
            &body.question,
            &body.context,
            &body.blocking_task_ids,
            routed_to,
            body.asked_by_pane_id.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    state
        .svc
        .deliver_gate_question(&id, &gate.id, &gate.question, body.asked_by_pane_id.as_deref());
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::to_value(&gate).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub(crate) struct GateListQuery {
    #[serde(default)]
    open_only: Option<bool>,
}

pub async fn gate_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<GateListQuery>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let gates = state
        .svc
        .plan_store
        .list_gates(&id, q.open_only.unwrap_or(false))
        .map_err(plan_err)?;
    Ok(Json(serde_json::to_value(&gates).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct GateAnswerBody {
    answer: String,
    #[serde(default)]
    answered_by_pane_id: Option<String>,
}

pub async fn gate_answer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, gid)): Path<(String, String)>,
    Json(body): Json<GateAnswerBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let gate = state
        .svc
        .plan_store
        .answer_gate(
            &id,
            &gid,
            &body.answer,
            body.answered_by_pane_id.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    state
        .svc
        .nudge_gate_asker(&id, &gid, &body.answer, body.answered_by_pane_id.as_deref());
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::to_value(&gate).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
pub(crate) struct GateEscalateBody {
    #[serde(default)]
    note: Option<String>,
}

pub async fn gate_escalate(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, gid)): Path<(String, String)>,
    Json(body): Json<GateEscalateBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    let gate = state
        .svc
        .plan_store
        .escalate_gate(&id, &gid, body.note.as_deref(), epoch_ms())
        .map_err(plan_err)?;
    emit_plan_changed(&state, &id);
    Ok(Json(serde_json::to_value(&gate).unwrap_or_default()))
}
