use serde_json::{json, Value};

use super::{get_string_param, JsonRpcError};
use crate::service::ServiceState;

fn error(message: impl Into<String>) -> JsonRpcError {
    let message = message.into();
    let version_conflict = message
        .strip_prefix("sentinel_version_conflict: current_version=")
        .and_then(|version| version.parse::<u64>().ok());
    let code = if version_conflict.is_some() {
        "sentinel_version_conflict"
    } else if message.starts_with("sentinel_store_unsupported_format:") {
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
        data: Some(match version_conflict {
            Some(current_version) => json!({
                "code": code,
                "current_version": current_version
            }),
            None => json!({"code": code}),
        }),
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
        "sentinel.validate.create" => {
            let create = params
                .as_ref()
                .and_then(|p| p.get("create"))
                .ok_or_else(|| error("create params required"))?;
            let create = serde_json::from_value(create.clone())
                .map_err(|e| error(format!("invalid sentinel params: {e}")))?;
            let mut preview = serde_json::to_value(
                svc.sentinel_validate_create(&caller, create)
                    .map_err(error)?,
            )
            .map_err(|e| error(e.to_string()))?;
            if let Some(fields) = preview.as_object_mut() {
                fields.remove("id");
                fields.remove("scenario_id");
            }
            Ok(preview)
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
        "sentinel.update" => {
            let id = get_string_param(params, "id")?;
            let expect_version = params
                .as_ref()
                .and_then(|value| value.get("expect_version"))
                .and_then(Value::as_u64)
                .ok_or_else(|| error("expect_version must be an unsigned integer"))?;
            let update = params
                .as_ref()
                .and_then(|value| value.get("update"))
                .ok_or_else(|| error("update params required"))?;
            let update = serde_json::from_value(update.clone())
                .map_err(|e| error(format!("invalid sentinel update: {e}")))?;
            serde_json::to_value(
                svc.sentinel_update(&caller, &id, expect_version, update)
                    .map_err(error)?,
            )
            .map_err(|e| error(e.to_string()))
        }
        "sentinel.validate.update" => {
            let id = get_string_param(params, "id")?;
            let expect_version = params
                .as_ref()
                .and_then(|value| value.get("expect_version"))
                .and_then(Value::as_u64)
                .ok_or_else(|| error("expect_version must be an unsigned integer"))?;
            let update = params
                .as_ref()
                .and_then(|value| value.get("update"))
                .ok_or_else(|| error("update params required"))?;
            let update = serde_json::from_value(update.clone())
                .map_err(|e| error(format!("invalid sentinel update: {e}")))?;
            serde_json::to_value(
                svc.sentinel_validate_update(&caller, &id, expect_version, update)
                    .map_err(error)?,
            )
            .map_err(|e| error(e.to_string()))
        }
        "sentinel.history" => {
            let id = get_string_param(params, "id")?;
            serde_json::to_value(svc.sentinel_history(&caller, &id).map_err(error)?)
                .map_err(|e| error(e.to_string()))
        }
        "sentinel.escalation.resolve" => {
            let id = get_string_param(params, "id")?;
            let transition_id = get_string_param(params, "transition_id")?;
            serde_json::to_value(
                svc.sentinel_resolve_escalation(&caller, &id, &transition_id)
                    .map_err(error)?,
            )
            .map_err(|e| error(e.to_string()))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_update_rpc_version_conflict_carries_current_version() {
        let error = error(crate::sentinel::amend::version_conflict(9));
        assert_eq!(
            error.data.as_ref().unwrap()["code"],
            "sentinel_version_conflict"
        );
        assert_eq!(error.data.as_ref().unwrap()["current_version"], 9);
    }
}
