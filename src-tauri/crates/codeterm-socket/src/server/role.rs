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

pub(super) fn dispatch(method: &str, params: &Option<Value>) -> Result<Value, JsonRpcError> {
    let team_id = field(params, "team_id");
    match method {
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
