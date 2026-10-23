use serde_json::{json, Value};

use crate::client::SocketClient;
use crate::local_api::LocalApiClient;

pub(super) fn routed_update(kind: &str, id: Option<&str>) -> Option<Result<Value, String>> {
    route(SocketClient::connect_optional_local(), |client| {
        let client = LocalApiClient::from_rpc(client);
        let mut result = client
            .post("/api/plugins/action")
            .json(&json!({"kind": kind, "pluginId": id}))
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.json::<Value>())
            .map_err(|error| error.to_string())
            .and_then(|snapshot| {
                snapshot
                    .get("lastOutput")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .ok_or_else(|| "Plugin update response did not contain its result".to_owned())
            })?;
        result["provider_registration"] = json!("acknowledged");
        Ok(result)
    })
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
