use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::JsonRpcError;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: e.rpc_code(),
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}

fn string_list(params: Option<&Value>, key: &str) -> Vec<String> {
    params
        .and_then(|x| x.get(key))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn opt_str<'a>(params: Option<&'a Value>, key: &str) -> Option<&'a str> {
    params.and_then(|x| x.get(key)).and_then(|v| v.as_str())
}

fn to_value(v: impl serde::Serialize) -> Result<Value, JsonRpcError> {
    serde_json::to_value(v).map_err(|e| JsonRpcError {
        code: -32603,
        message: e.to_string(),
        data: None,
    })
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    let now = epoch_ms();

    match method {
        "plan.decision.ask" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let question = opt_str(p, "question").unwrap_or("").trim().to_string();
            let context = opt_str(p, "context").unwrap_or("").to_string();
            let blocking = string_list(p, "blocking_task_ids");
            let asked_by = opt_str(p, "asked_by_tab_id")
                .or_else(|| opt_str(p, "caller_tab_id"))
                .map(str::to_string);

            if let Some(owner) = svc.planner_gate_owner_answerer(&plan_id, asked_by.as_deref()) {
                let gate = store
                    .ask_gate(
                        &plan_id,
                        &question,
                        &context,
                        &blocking,
                        Some(owner.clone()),
                        asked_by.as_deref(),
                        now,
                    )
                    .map_err(plan_rpc_err)?;
                svc.deliver_planner_gate_to_owner(&plan_id, &owner, &gate)
                    .map_err(|message| JsonRpcError {
                        code: -32003,
                        message,
                        data: Some(json!({
                            "type": "owner_gate_delivery_failed",
                            "gate": gate.clone()
                        })),
                    })?;
                emit_plan_changed(app, &plan_id);
                return to_value(&gate);
            }

            // Resolve where to route before storing, so heartbeat shows the routed tab.
            let routed_to = {
                let plan = store.get(&plan_id);
                plan.and_then(|pl| {
                    let resolution =
                        crate::plan::peer::resolve_planner_ref(&pl.planner, &pl.owner, |pid| {
                            svc.tab_probe(pid)
                        });
                    match resolution {
                        crate::plan::peer::PeerResolution::Planner { tab_id } => Some(tab_id),
                        crate::plan::peer::PeerResolution::OrchestratorFallback { .. }
                        | crate::plan::peer::PeerResolution::Waived { .. }
                        | crate::plan::peer::PeerResolution::Unbound => None,
                    }
                })
            };

            let caller_session_id = asked_by.as_deref().and_then(|tab_id| {
                svc.mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.agent_session_id.clone())
            });
            let (gate, request) = store
                .ask_gate_with_planner_request(
                    &plan_id,
                    &question,
                    &context,
                    &blocking,
                    routed_to.clone(),
                    asked_by.as_deref(),
                    caller_session_id.as_deref(),
                    now,
                )
                .map_err(plan_rpc_err)?;
            let request_id = request.request_id.clone();
            if let Some(tab_id) = routed_to.as_deref() {
                svc.replay_planner_requests_if_routable(tab_id);
            }
            let request = store
                .get(&plan_id)
                .and_then(|plan| {
                    plan.planner_requests
                        .into_iter()
                        .find(|item| item.request_id == request_id)
                })
                .unwrap_or(request);
            if !matches!(
                request.state,
                crate::plan::PlannerRequestState::Accepted
                    | crate::plan::PlannerRequestState::Completed
            ) {
                let plan = store.get(&plan_id);
                let resolution = plan.as_ref().map(|plan| {
                    crate::plan::peer::resolve_planner_ref(&plan.planner, &plan.owner, |tab_id| {
                        svc.tab_probe(tab_id)
                    })
                });
                let delivery = plan.as_ref().and_then(|plan| {
                    plan.planner_inbox
                        .iter()
                        .find(|entry| entry.request_id == request.request_id)
                });
                let planner_dead = matches!(
                    resolution,
                    Some(crate::plan::peer::PeerResolution::OrchestratorFallback { .. })
                );
                let recovery_action = if delivery.is_some_and(|entry| {
                    entry.phase == crate::plan::PlannerDeliveryPhase::DeliveryUncertain
                }) {
                    "resolve_delivery_uncertain"
                } else {
                    "replace_or_rebind_planner"
                };
                emit_plan_changed(app, &plan_id);
                return Err(JsonRpcError {
                    code: -32003,
                    message: "agent_unavailable".into(),
                    data: Some(json!({
                        "request_id": request.request_id,
                        "state": request.state,
                        "request": request,
                        "delivery": delivery,
                        "planner_resolution": resolution,
                        "planner_dead": planner_dead,
                        "recovery_action": recovery_action
                    })),
                });
            }
            emit_plan_changed(app, &plan_id);
            to_value(&gate)
        }

        "plan.decision.answer" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let gate_id = super::get_string_param(params, "gate_id")?;
            let answer = opt_str(p, "answer").unwrap_or("").to_string();
            let answered_by = opt_str(p, "answered_by_tab_id")
                .or_else(|| opt_str(p, "caller_tab_id"))
                .map(str::to_string);
            let gate = store
                .answer_gate(&plan_id, &gate_id, &answer, answered_by.as_deref(), now)
                .map_err(plan_rpc_err)?;
            svc.nudge_gate_asker(&plan_id, &gate_id, &answer, answered_by.as_deref());
            emit_plan_changed(app, &plan_id);
            to_value(&gate)
        }

        "plan.decision.ack_request" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let request_id = super::get_string_param(params, "request_id")?;
            let tab_id = opt_str(p, "caller_tab_id").ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "caller_tab_id is required".into(),
                data: None,
            })?;
            let request = svc
                .acknowledge_planner_request(&plan_id, &request_id, tab_id)
                .map_err(|message| JsonRpcError {
                    code: -32003,
                    message,
                    data: Some(json!({ "request_id": request_id })),
                })?;
            emit_plan_changed(app, &plan_id);
            to_value(request)
        }

        "plan.decision.list" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let open_only = p
                .and_then(|x| x.get("open_only"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let gates = store
                .list_gates(&plan_id, open_only)
                .map_err(plan_rpc_err)?;
            to_value(&gates)
        }

        "plan.decision.escalate" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let gate_id = super::get_string_param(params, "gate_id")?;
            let note = opt_str(p, "note").map(str::to_string);
            let gate = store
                .escalate_gate(&plan_id, &gate_id, note.as_deref(), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            to_value(&gate)
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: format!("Unknown plan decision method: {method}"),
            data: None,
        }),
    }
}
