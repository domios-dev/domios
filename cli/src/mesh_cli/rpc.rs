use crate::client::{RpcClient, SocketCallError};

pub struct MeshRpcClient {
    target: String,
}

impl MeshRpcClient {
    pub fn connect(_base: &str, _token: &Option<String>, target: &str) -> Result<Self, String> {
        crate::build_local_api().map_err(|e| format!("failed to connect to local API: {e}"))?;
        Ok(Self {
            target: target.to_owned(),
        })
    }
}

impl RpcClient for MeshRpcClient {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, SocketCallError> {
        let body = serde_json::json!({
            "method": method,
            "params": params,
            "request_id": uuid::Uuid::new_v4().to_string(),
        });
        let response = super::relay::mesh_relay_invoke_result(
            &self.target,
            "POST",
            "/api/mesh/rpc",
            Some(&body.to_string()),
        )
        .map_err(|failure| SocketCallError::Transport(failure.to_string()))?;
        parse_rpc_data(response)
    }

    fn is_remote(&self) -> bool {
        true
    }
}

#[cfg(test)]
fn parse_relay_response(response: serde_json::Value) -> Result<serde_json::Value, SocketCallError> {
    if response.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err(SocketCallError::Transport(
            response
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("mesh RPC failed")
                .to_string(),
        ));
    }
    let data = response
        .get("data")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if let Some(error) = data.get("error") {
        let error = serde_json::from_value(error.clone()).map_err(|e| {
            SocketCallError::Transport(format!("invalid mesh RPC error response: {e}"))
        })?;
        return Err(SocketCallError::Rpc(error));
    }
    Ok(data
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

fn parse_rpc_data(data: serde_json::Value) -> Result<serde_json::Value, SocketCallError> {
    if let Some(error) = data.get("error") {
        let error = serde_json::from_value(error.clone()).map_err(|e| {
            SocketCallError::Transport(format!("invalid mesh RPC error response: {e}"))
        })?;
        return Err(SocketCallError::Rpc(error));
    }
    Ok(data
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

#[cfg(test)]
mod tests {
    use super::parse_relay_response;
    use crate::client::SocketCallError;

    #[test]
    fn relay_result_preserves_rpc_payload() {
        let result = parse_relay_response(serde_json::json!({
            "ok": true,
            "data": {"result": {"plan_id": "pl_remote"}},
        }))
        .unwrap();
        assert_eq!(result["plan_id"], "pl_remote");
    }

    #[test]
    fn relay_rpc_error_preserves_code_and_data() {
        let error = parse_relay_response(serde_json::json!({
            "ok": true,
            "data": {
                "error": {"code": -32602, "message": "bad params", "data": {"field": "plan_id"}}
            },
        }))
        .unwrap_err();
        let SocketCallError::Rpc(error) = error else {
            panic!("expected RPC error");
        };
        assert_eq!(error.code, -32602);
        assert_eq!(error.data.unwrap()["field"], "plan_id");
    }
}
