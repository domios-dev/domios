use axum::{
    extract::{Path, Query, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::routes::AppState;
use codeterm_service_core::plan::peer::{resolve_planner_ref, PeerResolution};
use codeterm_service_core::service::plan::PlannerGateRead;

use super::{emit_plan_changed, epoch_ms, plan_err, text_err, PlanRouteResult};

fn resolve_routed_tab(state: &AppState, plan: &PlannerGateRead) -> Option<String> {
    let resolution =
        resolve_planner_ref(&plan.planner, &plan.owner, |pid| state.svc.tab_probe(pid));
    match resolution {
        PeerResolution::Planner { tab_id } => Some(tab_id),
        PeerResolution::OrchestratorFallback { .. }
        | PeerResolution::Waived { .. }
        | PeerResolution::Unbound => None,
    }
}

#[derive(Deserialize)]
pub struct GateAskBody {
    question: String,
    #[serde(default)]
    context: String,
    #[serde(default)]
    blocking_task_ids: Vec<String>,
    #[serde(default)]
    asked_by_tab_id: Option<String>,
}

pub async fn gate_ask(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut body): Json<GateAskBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        body.asked_by_tab_id =
            crate::auth::caller_claim(&state, &headers, body.asked_by_tab_id.as_deref())
                .map_err(|(status, message)| text_err(status, message))?;
        let gate_context = state
            .svc
            .plan_store
            .planner_gate_context(&id, None)
            .map_err(plan_err)?;
        if let Some(owner) = gate_context
            .as_ref()
            .and_then(|read| read.owner_for_gate(body.asked_by_tab_id.as_deref()))
            .map(str::to_string)
        {
            let gate = state
                .svc
                .plan_store
                .ask_gate(
                    &id,
                    &body.question,
                    &body.context,
                    &body.blocking_task_ids,
                    Some(owner.clone()),
                    body.asked_by_tab_id.as_deref(),
                    epoch_ms(),
                )
                .map_err(plan_err)?;
            let delivery = state.svc.deliver_planner_gate_to_owner(&id, &owner, &gate);
            emit_plan_changed(&state, &id);
            if let Err(error) = delivery {
                return Err((
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "agent_unavailable",
                        "detail": error,
                        "gate": gate
                    })),
                ));
            }
            return Ok(Json(serde_json::to_value(&gate).unwrap_or_default()));
        }
        let routed_to = gate_context
            .as_ref()
            .and_then(|read| resolve_routed_tab(&state, read));
        let caller_session_id = body.asked_by_tab_id.as_deref().and_then(|tab_id| {
            state
                .svc
                .mux
                .read()
                .get_tab(tab_id)
                .and_then(|tab| tab.agent_session_id.clone())
        });
        let (gate, request) = state
            .svc
            .plan_store
            .ask_gate_with_planner_request(
                &id,
                &body.question,
                &body.context,
                &body.blocking_task_ids,
                routed_to.clone(),
                body.asked_by_tab_id.as_deref(),
                caller_session_id.as_deref(),
                epoch_ms(),
            )
            .map_err(plan_err)?;
        let request_id = request.request_id.clone();
        if let Some(tab_id) = routed_to.as_deref() {
            state.svc.replay_planner_requests_if_routable(tab_id);
        }
        let gate_context = state
            .svc
            .plan_store
            .planner_gate_context(&id, Some(&request_id))
            .map_err(plan_err)?;
        let request = gate_context
            .as_ref()
            .and_then(|read| read.request.clone())
            .unwrap_or(request);
        if !matches!(
            request.state,
            codeterm_service_core::plan::PlannerRequestState::Accepted
                | codeterm_service_core::plan::PlannerRequestState::Completed
        ) {
            let resolution = gate_context.as_ref().map(|plan| {
                resolve_planner_ref(&plan.planner, &plan.owner, |tab_id| {
                    state.svc.tab_probe(tab_id)
                })
            });
            let delivery = gate_context
                .as_ref()
                .and_then(|read| read.delivery.as_ref());
            let planner_dead = matches!(
                resolution,
                Some(PeerResolution::OrchestratorFallback { .. })
            );
            let recovery_action = if delivery.is_some_and(|entry| {
                entry.phase == codeterm_service_core::plan::PlannerDeliveryPhase::DeliveryUncertain
            }) {
                "resolve_delivery_uncertain"
            } else {
                "replace_or_rebind_planner"
            };
            emit_plan_changed(&state, &id);
            return Err((
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "agent_unavailable",
                    "request_id": request.request_id,
                    "state": request.state,
                    "request": request,
                    "delivery": delivery,
                    "planner_resolution": resolution,
                    "planner_dead": planner_dead,
                    "recovery_action": recovery_action
                })),
            ));
        }
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::to_value(&gate).unwrap_or_default()))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct GateListQuery {
    #[serde(default)]
    open_only: Option<bool>,
}

pub async fn gate_list(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<GateListQuery>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let gates = state
            .svc
            .plan_store
            .list_gates(&id, q.open_only.unwrap_or(false))
            .map_err(plan_err)?;
        Ok(Json(serde_json::to_value(&gates).unwrap_or_default()))
    })
    .await
}

#[derive(Deserialize)]
pub struct GateAnswerBody {
    answer: String,
    #[serde(default)]
    answered_by_tab_id: Option<String>,
}

pub async fn gate_answer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path((id, gid)): Path<(String, String)>,
    Json(mut body): Json<GateAnswerBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        body.answered_by_tab_id =
            crate::auth::caller_claim(&state, &headers, body.answered_by_tab_id.as_deref())
                .map_err(|(status, message)| text_err(status, message))?;
        let gate = state
            .svc
            .plan_store
            .answer_gate(
                &id,
                &gid,
                &body.answer,
                body.answered_by_tab_id.as_deref(),
                epoch_ms(),
            )
            .map_err(plan_err)?;
        state
            .svc
            .nudge_gate_asker(&id, &gid, &body.answer, body.answered_by_tab_id.as_deref());
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::to_value(&gate).unwrap_or_default()))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct GateEscalateBody {
    #[serde(default)]
    note: Option<String>,
}

pub async fn gate_escalate(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((id, gid)): Path<(String, String)>,
    Json(body): Json<GateEscalateBody>,
) -> PlanRouteResult {
    crate::plan_routes::run_blocking(move || {
        let gate = state
            .svc
            .plan_store
            .escalate_gate(&id, &gid, body.note.as_deref(), epoch_ms())
            .map_err(plan_err)?;
        emit_plan_changed(&state, &id);
        Ok(Json(serde_json::to_value(&gate).unwrap_or_default()))
    })
    .await
}
