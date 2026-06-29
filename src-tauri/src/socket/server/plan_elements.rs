use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn handles(method: &str) -> bool {
    matches!(method, "plan.layout.set" | "plan.note.add") || method.starts_with("plan.element.")
}

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
    let p = params.as_ref();
    let store = &svc.plan_store;
    let now = epoch_ms();

    match method {
        "plan.layout.set" => {
            let id = get_string_param(params, "plan_id")?;
            let element = get_string_param(params, "element")?;
            store
                .set_layout(
                    &id,
                    &element,
                    p.and_then(|x| x.get("x")).and_then(|v| v.as_f64()),
                    p.and_then(|x| x.get("y")).and_then(|v| v.as_f64()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }
        "plan.note.add" => {
            let id = get_string_param(params, "plan_id")?;
            let text = p
                .and_then(|x| x.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let links = p
                .and_then(|x| x.get("links"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let eid = store
                .add_note(
                    &id,
                    text,
                    links,
                    p.and_then(|x| x.get("x")).and_then(|v| v.as_f64()),
                    p.and_then(|x| x.get("y")).and_then(|v| v.as_f64()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "element_id": eid }))
        }
        "plan.element.add" => {
            let id = get_string_param(params, "plan_id")?;
            let kind = get_string_param(params, "kind")?;
            let content = p
                .and_then(|x| x.get("content"))
                .cloned()
                .unwrap_or(json!({}));
            let links = p
                .and_then(|x| x.get("links"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let eid = store
                .add_element(
                    &id,
                    &kind,
                    content,
                    links,
                    p.and_then(|x| x.get("x")).and_then(|v| v.as_f64()),
                    p.and_then(|x| x.get("y")).and_then(|v| v.as_f64()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "element_id": eid }))
        }
        "plan.element.list" => {
            let id = get_string_param(params, "plan_id")?;
            let kind = p.and_then(|x| x.get("kind")).and_then(|v| v.as_str());
            let result = store.list_elements(&id, kind).map_err(plan_rpc_err)?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.element.show" => {
            let id = get_string_param(params, "plan_id")?;
            let element_id = get_string_param(params, "element_id")?;
            let result = store.show_element(&id, &element_id).map_err(plan_rpc_err)?;
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
