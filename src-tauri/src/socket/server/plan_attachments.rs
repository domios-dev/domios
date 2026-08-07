use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{get_string_param, plan_refs, JsonRpcError};

pub(super) fn handles(method: &str) -> bool {
    matches!(method, "plan.section.attach" | "plan.task.attach")
}

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

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let now = epoch_ms();
    match method {
        "plan.section.attach" => {
            let (id, sid) = plan_refs::resolve_section(store, params)?;
            let path = get_string_param(params, "path")?;
            let attachment = store
                .attach_to_section(&id, &sid, std::path::Path::new(&path), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "attachment": attachment }))
        }
        "plan.task.attach" => {
            let (id, tid) = plan_refs::resolve_task(store, params)?;
            let path = get_string_param(params, "path")?;
            let attachment = store
                .attach_to_task(&id, &tid, std::path::Path::new(&path), now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "attachment": attachment }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "method not found".into(),
            data: None,
        }),
    }
}
