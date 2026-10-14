//! `persona.*` JSON-RPC handlers — read-only inspection of the shared role
//! preset resolver plus an explicit overlay reload.

use serde_json::Value;

use codeterm_types::agent::AgentRole;

use crate::service::persona::{inspect, occasion::Occasion, overlay};

use super::JsonRpcError;

fn invalid(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: message.into(),
        data: None,
    }
}

fn encode(value: impl serde::Serialize) -> Result<Value, JsonRpcError> {
    serde_json::to_value(value).map_err(|error| JsonRpcError {
        code: -32603,
        message: error.to_string(),
        data: None,
    })
}

fn field<'a>(params: &'a Option<Value>, key: &str) -> Option<&'a str> {
    params
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn dispatch(method: &str, params: &Option<Value>) -> Result<Value, JsonRpcError> {
    match method {
        "persona.list" => encode(inspect::list(overlay::ambient().as_ref())),
        "persona.reload" => encode(inspect::list(overlay::reload().as_ref())),
        "persona.show" => {
            let role = field(params, "role").unwrap_or("Worker");
            let role = AgentRole::from_str_compat(role)
                .ok_or_else(|| invalid(format!("unknown role `{role}`")))?;
            let only = match field(params, "occasion") {
                Some(id) => Some(
                    Occasion::parse(id)
                        .ok_or_else(|| invalid(format!("unknown occasion `{id}`")))?,
                ),
                None => None,
            };
            let view = inspect::show(
                overlay::ambient().as_ref(),
                role,
                field(params, "kind"),
                field(params, "guidance"),
                only,
            )
            .ok_or_else(|| invalid("that role and kind resolve to no preset"))?;
            encode(view)
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
