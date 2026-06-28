use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

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

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let body = params.as_ref().ok_or(JsonRpcError {
        code: -32602,
        message: "params required".to_string(),
        data: None,
    })?;
    match method {
        "plan.heartbeat.learn" => {
            let text = get_string_param(params, "text")?;
            let result = svc
                .plan_store
                .add_heartbeat_learning(
                    body.get("plan_id").and_then(|v| v.as_str()),
                    body.get("pane_id").and_then(|v| v.as_str()),
                    body.get("session_id").and_then(|v| v.as_str()),
                    &text,
                    body.get("tag").and_then(|v| v.as_str()),
                    epoch_ms(),
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &result.plan_id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.heartbeat.set" => {
            let str_list = |key: &str| {
                body.get(key)
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
            };
            let bool_flag = |key: &str| body.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
            let result = svc
                .plan_store
                .set_heartbeat(
                    body.get("plan_id").and_then(|v| v.as_str()),
                    body.get("pane_id").and_then(|v| v.as_str()),
                    body.get("session_id").and_then(|v| v.as_str()),
                    body.get("focus").and_then(|v| v.as_str()),
                    str_list("lanes"),
                    str_list("rules"),
                    bool_flag("clear_lanes"),
                    bool_flag("clear_rules"),
                    epoch_ms(),
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &result.plan_id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.heartbeat.show" => {
            let result = svc
                .plan_store
                .show_heartbeat(
                    body.get("plan_id").and_then(|v| v.as_str()),
                    body.get("pane_id").and_then(|v| v.as_str()),
                    body.get("session_id").and_then(|v| v.as_str()),
                )
                .map_err(plan_rpc_err)?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
