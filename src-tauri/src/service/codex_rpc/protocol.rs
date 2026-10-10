use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[cfg(test)]
impl RpcError {
    pub fn unavailable(&self) -> bool {
        self.code == -32601
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    ServerRequest {
        id: RequestId,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: RequestId,
        result: Result<Value, RpcError>,
    },
}

pub fn decode(text: &str) -> Result<Frame, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let object = value.as_object().ok_or("RPC frame must be an object")?;
    let id = object
        .get("id")
        .map(|id| serde_json::from_value::<RequestId>(id.clone()))
        .transpose()
        .map_err(|error| error.to_string())?;
    if let Some(method) = object.get("method") {
        let method = method
            .as_str()
            .ok_or("RPC method must be a string")?
            .to_owned();
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        return Ok(match id {
            Some(id) => Frame::ServerRequest { id, method, params },
            None => Frame::Notification { method, params },
        });
    }
    let id = id.ok_or("RPC response is missing its id")?;
    let result = match (object.get("result"), object.get("error")) {
        (Some(result), None) => Ok(result.clone()),
        (None, Some(error)) => {
            Err(serde_json::from_value(error.clone()).map_err(|error| error.to_string())?)
        }
        _ => return Err("RPC response must contain exactly one of result or error".into()),
    };
    Ok(Frame::Response { id, result })
}

pub fn initialize_params() -> Value {
    serde_json::json!({
        "clientInfo": {"name": "domios", "version": env!("CARGO_PKG_VERSION")},
        "capabilities": {"experimentalApi": true, "optOutNotificationMethods": ["turn/diff/updated"]}
    })
}

#[cfg(test)]
mod tests;
