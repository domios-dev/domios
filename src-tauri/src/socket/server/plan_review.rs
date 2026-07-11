use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::AgentRef;
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
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str())
                .map(AgentRef::from_pane)
        })
        .unwrap_or_else(|| AgentRef::from_pane("unknown"))
}

fn note(params: Option<&Value>) -> &str {
    params
        .and_then(|x| x.get("note").or_else(|| x.get("text")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn bool_field(params: Option<&Value>, key: &str) -> bool {
    params
        .and_then(|x| x.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn str_field<'a>(params: Option<&'a Value>, key: &str) -> &'a str {
    params
        .and_then(|x| x.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or("")
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
        "plan.review.status" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let status = store.review_gate_status(&plan_id).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "review_gate": status }))
        }
        "plan.review.waves" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let waves = store.qa_waves(&plan_id).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "waves": waves }))
        }
        "plan.review.override" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            store
                .record_review_override(
                    &plan_id,
                    &task_id,
                    str_field(p, "owner"),
                    str_field(p, "reason"),
                    str_field(p, "reproduction"),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "plan_id": plan_id, "task_id": task_id, "recorded": true }))
        }
        "plan.review.accept" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let residual_risk = p
                .and_then(|x| x.get("residual_risk"))
                .and_then(|v| v.as_str());
            let result = store
                .accept_review(
                    &plan_id,
                    &task_id,
                    note(p),
                    bool_field(p, "changed_executable_surfaces"),
                    bool_field(p, "distinct_oracle"),
                    residual_risk,
                    actor(p),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.review.bounce" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let result = store
                .bounce_review(
                    &plan_id,
                    &task_id,
                    note(p),
                    bool_field(p, "changed_executable_surfaces"),
                    bool_field(p, "distinct_oracle"),
                    actor(p),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: format!("Unknown plan review method: {method}"),
            data: None,
        }),
    }
}
