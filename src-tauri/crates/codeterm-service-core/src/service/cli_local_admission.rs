use super::ServiceContext;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    Maintenance,
    HostOwner,
    LocalInput,
}

pub async fn authorize(svc: &ServiceContext, credential: &str, params: Option<&Value>) -> Result<Value, String> {
    let caller = svc.authenticate_local_credential(credential)?;
    let operation: Operation = serde_json::from_value(params.and_then(|value| value.get("operation")).cloned().unwrap_or(Value::Null))
        .map_err(|_| "Unknown local CLI operation".to_string())?;
    svc.authorize_neighbor_rpc_credential(credential, "cli.local.authorize", params).await?;
    if matches!(operation, Operation::HostOwner) && caller.is_some() {
        return Err("Local CLI operation requires the local owner credential".into());
    }
    Ok(json!({"authorized":true}))
}

#[cfg(test)]
mod tests;
