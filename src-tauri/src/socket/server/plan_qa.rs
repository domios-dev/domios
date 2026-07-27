use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::AgentRef;
use crate::service::messenger::MessageEnvelope;
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

fn actor(params: Option<&Value>) -> AgentRef {
    params
        .and_then(|x| x.get("from"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .or_else(|| {
            params
                .and_then(|x| x.get("caller_tab_id"))
                .and_then(|v| v.as_str())
                .map(AgentRef::from_tab)
        })
        .unwrap_or_else(|| AgentRef::from_tab("unknown"))
}

fn note(params: Option<&Value>) -> &str {
    params
        .and_then(|x| x.get("note").or_else(|| x.get("text")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn maybe_nudge_implementer(svc: &ServiceState, tab_id: Option<&str>, task_id: &str, note: &str) {
    let Some(tab_id) = tab_id.filter(|tab| !tab.trim().is_empty()) else {
        return;
    };
    let text = format!(
        "[qa failed] Task {task_id} needs changes.\n{note}\nUpdate the implementation, then report in_review again."
    );
    let envelope = MessageEnvelope::codeterm_reminder("qa");
    let _ = svc.gateway_deliver(
        tab_id,
        envelope,
        &text,
        crate::service::message_gateway::DeliveryClass::SystemNotice,
    );
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
        "plan.qa.queue" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let task = p.and_then(|x| x.get("task_id")).and_then(|v| v.as_str());
            if let Some(task_id) = task {
                let result = store
                    .enqueue_qa(&plan_id, task_id, note(p), actor(p), now)
                    .map_err(plan_rpc_err)?;
                emit_plan_changed(app, &plan_id);
                serde_json::to_value(result).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                })
            } else {
                serde_json::to_value(store.qa_queue(&plan_id, now).map_err(plan_rpc_err)?).map_err(
                    |e| JsonRpcError {
                        code: -32603,
                        message: e.to_string(),
                        data: None,
                    },
                )
            }
        }
        "plan.qa.next" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let group = p
                .and_then(|x| x.get("group"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if let Some(group) = group {
                let gate = store
                    .record_group_regression(&plan_id, group, note(p), actor(p), now)
                    .map_err(plan_rpc_err)?;
                emit_plan_changed(app, &plan_id);
                return Ok(json!({ "plan_id": plan_id, "regression_gate": gate }));
            }
            let claim = p.and_then(|x| x.get("claim")).and_then(|v| v.as_bool()) == Some(true);
            if !claim {
                let queue = store.qa_queue(&plan_id, now).map_err(plan_rpc_err)?;
                return Ok(json!({ "plan_id": plan_id, "next": queue.queued.first() }));
            }
            let tester = p
                .and_then(|x| x.get("tester_tab_id").or_else(|| x.get("caller_tab_id")))
                .and_then(|v| v.as_str());
            let result = store
                .claim_next_qa(&plan_id, note(p), actor(p), tester, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "plan_id": plan_id, "claimed": result }))
        }
        "plan.qa.override" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let gates = p
                .and_then(|x| x.get("gates"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let reason = p
                .and_then(|x| x.get("reason").or_else(|| x.get("note")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let record = store
                .record_regression_override(&plan_id, gates, reason, actor(p), now)
                .map_err(plan_rpc_err)?;
            let remaining = {
                let plan = store.get(&plan_id);
                plan.map(|pl| crate::service::plan::milestone_gates::blocking_regression_gates(&pl))
                    .unwrap_or_default()
            };
            emit_plan_changed(app, &plan_id);
            Ok(json!({
                "plan_id": plan_id,
                "override": record,
                "blocking_gates": remaining,
            }))
        }
        "plan.qa.pass" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let result = store
                .pass_qa(&plan_id, &task_id, note(p), actor(p), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.qa.fail" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let result = store
                .fail_qa(&plan_id, &task_id, note(p), actor(p), now)
                .map_err(plan_rpc_err)?;
            maybe_nudge_implementer(svc, result.nudged_tab_id.as_deref(), &task_id, note(p));
            emit_plan_changed(app, &plan_id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: format!("Unknown plan qa method: {method}"),
            data: None,
        }),
    }
}
