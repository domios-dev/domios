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
        code: -32603,
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
            let asked_by = opt_str(p, "asked_by_pane_id")
                .or_else(|| opt_str(p, "caller_pane_id"))
                .map(str::to_string);

            // Resolve where to route before storing, so heartbeat shows the routed pane.
            let routed_to = {
                let plan = store.get(&plan_id);
                plan.and_then(|pl| {
                    let resolution =
                        crate::plan::peer::resolve_planner_ref(&pl.planner, &pl.owner, |pid| {
                            svc.pane_probe(pid)
                        });
                    match resolution {
                        crate::plan::peer::PeerResolution::Planner { pane_id } => Some(pane_id),
                        crate::plan::peer::PeerResolution::OrchestratorFallback { .. } => {
                            pl.owner.pane_id.clone()
                        }
                        crate::plan::peer::PeerResolution::Waived { .. }
                        | crate::plan::peer::PeerResolution::Unbound => None,
                    }
                })
            };

            let gate = store
                .ask_gate(
                    &plan_id,
                    &question,
                    &context,
                    &blocking,
                    routed_to,
                    asked_by.as_deref(),
                    now,
                )
                .map_err(plan_rpc_err)?;
            svc.deliver_gate_question(&plan_id, &gate.id, &gate.question, asked_by.as_deref());
            emit_plan_changed(app, &plan_id);
            to_value(&gate)
        }

        "plan.decision.answer" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let gate_id = super::get_string_param(params, "gate_id")?;
            let answer = opt_str(p, "answer").unwrap_or("").to_string();
            let answered_by = opt_str(p, "answered_by_pane_id")
                .or_else(|| opt_str(p, "caller_pane_id"))
                .map(str::to_string);
            let gate = store
                .answer_gate(&plan_id, &gate_id, &answer, answered_by.as_deref(), now)
                .map_err(plan_rpc_err)?;
            svc.nudge_gate_asker(&plan_id, &gate_id, &answer, answered_by.as_deref());
            emit_plan_changed(app, &plan_id);
            to_value(&gate)
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
