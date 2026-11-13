use serde_json::{json, Value};

use crate::args::plugin::PluginAction;
use crate::client::SocketClient;
use crate::local_api::LocalApiClient;

pub(super) fn routed_action(
    action: &PluginAction,
    daemon: Option<SocketClient>,
) -> Option<Result<Value, String>> {
    route_action(action, Ok(daemon), |client, body| {
        let client = LocalApiClient::from_rpc(client);
        let mut result = client
            .post("/api/plugins/action")
            .json(&body)
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.json::<Value>())
            .map_err(|error| error.to_string())
            .and_then(|snapshot| {
                snapshot
                    .get("lastOutput")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .ok_or_else(|| "Plugin action response did not contain its result".to_owned())
            })?;
        result["provider_registration"] = json!("acknowledged");
        Ok(result)
    })
}

fn route_action<T>(
    action: &PluginAction,
    connection: Result<Option<T>, String>,
    request: impl FnOnce(T, Value) -> Result<Value, String>,
) -> Option<Result<Value, String>> {
    let body = match action {
        PluginAction::Install {
            id,
            from_dir,
            id_override,
            channel,
        } => {
            let source = from_dir
                .as_ref()
                .map(|directory| {
                    std::path::absolute(directory)
                        .map(|path| path.to_string_lossy().into_owned())
                        .map_err(|error| format!("Resolve plugin source directory: {error}"))
                })
                .transpose();
            let source = match source {
                Ok(source) => source,
                Err(error) => return Some(Err(error)),
            };
            json!({"kind":"install", "pluginId":id, "fromDir":source,
                "idOverride":id_override, "channel":channel})
        }
        PluginAction::Update { id } => json!({"kind":"update", "pluginId":id}),
        PluginAction::UpdateSystem { id } => json!({"kind":"update-system", "pluginId":id}),
        _ => return None,
    };
    route(connection, |client| request(client, body))
}

fn route<T>(
    connection: Result<Option<T>, String>,
    request: impl FnOnce(T) -> Result<Value, String>,
) -> Option<Result<Value, String>> {
    match connection {
        Ok(Some(client)) => Some(request(client)),
        Ok(None) => None,
        Err(error) => Some(Err(error)),
    }
}

#[cfg(test)]
mod tests;
