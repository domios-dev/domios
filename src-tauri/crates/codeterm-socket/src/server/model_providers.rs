use super::JsonRpcError;
use codeterm_service_core::service::{model_providers::ModelProviderRequest, ServiceState};
use serde_json::{json, Value};

pub(super) fn dispatch(
    method: &str,
    params: Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let action = match method {
        "model_provider.preset.add" => "preset_add",
        "model_provider.preset.remove" => "preset_remove",
        "model_provider.preset.list" => "preset_list",
        "model_provider.add" => "add",
        "model_provider.add_from_secret" => "add_from_secret",
        "model_provider.update" => "update",
        "model_provider.discover" => "discover",
        "model_provider.remove" => "remove",
        "model_provider.list" => "list",
        "model_provider.test" => "test",
        _ => {
            return Err(JsonRpcError {
                code: -32601,
                message: "unknown model provider operation".into(),
                data: None,
            })
        }
    };
    let mut params = params.unwrap_or_else(|| json!({}));
    let object = params.as_object_mut().ok_or_else(invalid)?;
    let provider = match object.remove("provider") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => Some(id),
        Some(_) => return Err(invalid()),
    };
    object.insert("action".into(), json!(action));
    let request: ModelProviderRequest = serde_json::from_value(params).map_err(|_| invalid())?;
    svc.model_provider_command(provider.as_deref(), request)
        .map_err(|error| JsonRpcError {
            code: -32000,
            message: error.to_string(),
            data: Some(json!({ "kind": error })),
        })
}

fn invalid() -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: "invalid model provider request".into(),
        data: None,
    }
}
