//! `plugin.receiver_target.*` JSON-RPC handlers — the CLI's route to host-owned receiver targets.

use serde_json::Value;

use crate::service::receiver_target::ReceiverTargetError;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

const METHOD_PREFIX: &str = "plugin.receiver_target.";

pub(super) fn handles(method: &str) -> bool {
    method.starts_with(METHOD_PREFIX)
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let plugin_id = get_string_param(params, "plugin_id")?;
    let result = match method.strip_prefix(METHOD_PREFIX) {
        Some("get") => svc.receiver_target(&plugin_id),
        Some("set") => {
            let tab_id = get_string_param(params, "tab_id")?;
            svc.set_receiver_target(&plugin_id, &tab_id)
        }
        Some("clear") => svc.clear_receiver_target(&plugin_id),
        _ => {
            return Err(JsonRpcError {
                code: -32601,
                message: format!("Unknown method: {method}"),
                data: None,
            })
        }
    };
    result
        .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
        .map_err(rpc_error)
}

fn rpc_error(error: ReceiverTargetError) -> JsonRpcError {
    let code = match error {
        ReceiverTargetError::PersistFailed => -32603,
        ReceiverTargetError::ReceiverNotFound
        | ReceiverTargetError::TabNotFound
        | ReceiverTargetError::MenuItemNotFound => -32602,
    };
    JsonRpcError {
        code,
        message: error.message().to_string(),
        data: Some(serde_json::json!({ "code": error.code() })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pebble_receiver_target_socket_methods_never_reach_agent_commands() {
        assert!(handles("plugin.receiver_target.get"));
        assert!(handles("plugin.receiver_target.set"));
        assert!(!handles("plugin:receiver.receiver_target"));
        assert!(!handles("plugin:npc.say"));

        let refused = rpc_error(ReceiverTargetError::TabNotFound);
        assert_eq!(refused.code, -32602);
        assert_eq!(
            refused.data,
            Some(serde_json::json!({ "code": "tab_not_found" }))
        );
    }
}
