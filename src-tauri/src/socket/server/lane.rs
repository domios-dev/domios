use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    _app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let subject = get_string_param(params, "subject")?;
    let plan_ref = params
        .as_ref()
        .and_then(|value| value.get("plan_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    match method {
        "lane.check" => {
            let (plan_id, task_id, diagnostics) = svc
                .check_lane_subject(plan_ref, &subject)
                .map_err(internal_error)?;
            Ok(json!({
                "ok": true,
                "plan_id": plan_id,
                "task_id": task_id,
                "diagnostics": diagnostics,
            }))
        }
        "lane.diagnostics" => {
            let resolved = svc
                .plan_store
                .resolve_task_ref(plan_ref, &subject)
                .map_err(|error| invalid_params(&error.to_string()))?;
            let diagnostics = svc
                .diagnostics_for_task(&resolved.plan_id, &resolved.task_id)
                .map_err(internal_error)?;
            Ok(json!({
                "ok": true,
                "plan_id": resolved.plan_id,
                "task_id": resolved.task_id,
                "diagnostics": diagnostics,
            }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        }),
    }
}

fn invalid_params(message: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: message.into(),
        data: None,
    }
}

fn internal_error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message,
        data: None,
    }
}
