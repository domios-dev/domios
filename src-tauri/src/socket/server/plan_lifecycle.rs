use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::AgentRef;
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "plan.task.park" | "plan.task.unpark" | "plan.task.close"
    )
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let now = epoch_ms();
    match method {
        "plan.task.park" => {
            let (id, tid) = super::plan_refs::resolve_task(store, params)?;
            let reason = get_string_param(params, "reason")?;
            let milestone_id = store
                .park_task(&id, &tid, &reason, from_param(p), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id, "task_id": tid, "milestone_id": milestone_id }))
        }
        "plan.task.unpark" => {
            let (id, tid) = super::plan_refs::resolve_task(store, params)?;
            let milestone_id = store
                .unpark_task(&id, &tid, from_param(p), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id, "task_id": tid, "milestone_id": milestone_id }))
        }
        "plan.task.close" => {
            let (id, tid) = super::plan_refs::resolve_task(store, params)?;
            store
                .close_task(
                    &id,
                    &tid,
                    p.and_then(|x| x.get("result")).and_then(|v| v.as_str()),
                    from_param(p),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn from_param(p: Option<&Value>) -> AgentRef {
    p.and_then(|x| x.get("from"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| AgentRef::from_pane("unknown"))
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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
