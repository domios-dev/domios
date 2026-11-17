use serde_json::Value;

use codeterm_service_core::service::persona::{inspect, occasion::Occasion};
use codeterm_service_core::service::team_roles::TeamRoleStore;

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

fn listing(team_id: Option<&str>) -> Result<Value, JsonRpcError> {
    let team = codeterm_service_core::service::team_roles::registrations(team_id);
    encode(inspect::list(team_id, &team))
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &codeterm_service_core::service::ServiceState,
) -> Result<Value, JsonRpcError> {
    let team_id = field(params, "team_id");
    match method {
        "role.sync" => {
            let team = team_id.ok_or_else(|| invalid("team_id is required"))?;
            let role = field(params, "role").ok_or_else(|| invalid("role is required"))?;
            let preview = params
                .as_ref()
                .and_then(|p| p.get("preview"))
                .and_then(Value::as_bool)
                .ok_or_else(|| invalid("preview must be a boolean"))?;
            if preview {
                svc.preview_role_sync(team, role).map_err(invalid)
            } else {
                let current = field(params, "current_hash")
                    .ok_or_else(|| invalid("current_hash is required"))?;
                let replacement = field(params, "replacement_hash")
                    .ok_or_else(|| invalid("replacement_hash is required"))?;
                svc.request_role_sync(team, role, current, replacement)
                    .map_err(invalid)
            }
        }
        "role.list" => listing(team_id),
        "role.reload" => {
            TeamRoleStore::global().reload(team_id);
            listing(team_id)
        }
        "role.show" => {
            let value = field(params, "type").unwrap_or("worker");
            let role_type = TeamRoleStore::global()
                .role_type(team_id, value)
                .map_err(|error| invalid(error.to_string()))?;
            let only = match field(params, "occasion") {
                Some(id) => Some(
                    Occasion::parse(id)
                        .ok_or_else(|| invalid(format!("unknown occasion `{id}`")))?,
                ),
                None => None,
            };
            let team = codeterm_service_core::service::team_roles::registrations(team_id);
            let view = inspect::show(&team, &role_type, field(params, "guidance"), only)
                .ok_or_else(|| invalid("that role type resolves to no preset"))?;
            encode(view)
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
