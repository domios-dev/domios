use axum::{
    extract::{Path, Query, State as AxumState},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::plan::peer::{resolve_planner_ref, PeerResolution};

use super::{emit_plan_changed, epoch_ms, plan_err, text_err, PlanRouteResult};

fn resolve_routed_tab(state: &AppState, plan_id: &str) -> Option<String> {
    let plan = state.svc.plan_store.get(plan_id)?;
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
pub(crate) struct GateAskBody {
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
    Json(body): Json<GateAskBody>,
) -> PlanRouteResult {
    check_auth(&state, &headers).map_err(|(s, m)| text_err(s, m))?;
    if let Some(owner) = state
        .svc
        .planner_gate_owner_answerer(&id, body.asked_by_tab_id.as_deref())
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
        let svc = state.svc.clone();
        let plan_id = id.clone();
        let delivered_gate = gate.clone();
        let delivery = tokio::task::spawn_blocking(move || {
            svc.deliver_planner_gate_to_owner(&plan_id, &owner, &delivered_gate)
        })
        .await;
        emit_plan_changed(&state, &id);
        let delivery = delivery.map_err(|error| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
        })?;
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
    let routed_to = resolve_routed_tab(&state, &id);
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
        let svc = state.svc.clone();
        let tab_id = tab_id.to_string();
        let _ =
            tokio::task::spawn_blocking(move || svc.replay_planner_requests_if_routable(&tab_id))
                .await;
    }
    let request = state
        .svc
        .plan_store
        .get(&id)
        .and_then(|plan| {
            plan.planner_requests
                .into_iter()
                .find(|item| item.request_id == request_id)
        })
        .unwrap_or(request);
    if !matches!(
        request.state,
        crate::plan::PlannerRequestState::Accepted | crate::plan::PlannerRequestState::Completed
    ) {
        let plan = state.svc.plan_store.get(&id);
        let resolution = plan.as_ref().map(|plan| {
            resolve_planner_ref(&plan.planner, &plan.owner, |tab_id| {
                state.svc.tab_probe(tab_id)
            })
        });
        let delivery = plan.as_ref().and_then(|plan| {
            plan.planner_inbox
                .iter()
                .find(|entry| entry.request_id == request.request_id)
        });
        let planner_dead = matches!(
            resolution,
            Some(PeerResolution::OrchestratorFallback { .. })
        );
        let recovery_action = if delivery.is_some_and(|entry| {
            entry.phase == crate::plan::PlannerDeliveryPhase::DeliveryUncertain
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
    answered_by_tab_id: Option<String>,
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
            body.answered_by_tab_id.as_deref(),
            epoch_ms(),
        )
        .map_err(plan_err)?;
    state
        .svc
        .nudge_gate_asker(&id, &gid, &body.answer, body.answered_by_tab_id.as_deref());
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
