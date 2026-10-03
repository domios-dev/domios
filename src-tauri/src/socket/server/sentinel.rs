use serde_json::{json, Value};

use super::{get_string_param, JsonRpcError};
use crate::service::ServiceState;

fn error(message: impl Into<String>) -> JsonRpcError {
    let message = message.into();
    let code = if message.starts_with("sentinel_store_unsupported_format:") {
        "sentinel_store_unsupported_format"
    } else if message.starts_with("sentinel_store_load_failed:") {
        "sentinel_store_load_failed"
    } else if message.starts_with("sentinel_store_persistence_failed:") {
        "sentinel_store_persistence_failed"
    } else {
        "sentinel_refused"
    };
    JsonRpcError {
        code: -32602,
        message,
        data: Some(json!({"code": code})),
    }
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let tab = get_string_param(params, "caller_tab_id")?;
    let nonce = params
        .as_ref()
        .and_then(|p| p.get("session_binding_nonce"))
        .and_then(Value::as_str);
    let caller = svc.sentinel_caller(&tab, nonce).map_err(error)?;
    match method {
        "sentinel.create" => {
            let create = params
                .as_ref()
                .and_then(|p| p.get("create"))
                .ok_or_else(|| error("create params required"))?;
            let create = serde_json::from_value(create.clone())
                .map_err(|e| error(format!("invalid sentinel params: {e}")))?;
            serde_json::to_value(svc.sentinel_create(&caller, create).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "sentinel.list" => serde_json::to_value(svc.sentinel_list(&caller).map_err(error)?)
            .map_err(|e| error(e.to_string())),
        "sentinel.show" => {
            let id = get_string_param(params, "id")?;
            serde_json::to_value(svc.sentinel_show(&caller, &id).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "sentinel.stop" => {
            let id = get_string_param(params, "id")?;
            serde_json::to_value(svc.sentinel_stop(&caller, &id).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        }),
    }
}
