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
        let body = rpc_request_body(method, params);
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

fn rpc_request_body(method: &str, params: Option<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "method": method,
        "params": params,
        "request_id": uuid::Uuid::new_v4().to_string(),
        "protocol_version": crate::protocol::PROTOCOL_VERSION,
    })
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
    use super::{parse_relay_response, parse_rpc_data, rpc_request_body};
    use crate::client::SocketCallError;

    #[test]
    fn version_skew_mesh_request_presents_the_protocol_integer() {
        let body = rpc_request_body("plan.get", Some(serde_json::json!({"plan_id": "pl_1"})));

        assert_eq!(body["protocol_version"], crate::protocol::PROTOCOL_VERSION);
        assert_eq!(body["method"], "plan.get");
        assert_eq!(body["params"]["plan_id"], "pl_1");
    }

    #[test]
    fn version_skew_error_crosses_the_relay_untranslated() {
        let message = "client protocol v2, server requires >=6 — run recover";
        let peer_response = serde_json::json!({
            "error": {
                "code": crate::protocol::VERSION_SKEW_CODE,
                "message": message,
                "data": {
                    "type": "version_skew",
                    "client_protocol": 2,
                    "server_protocol": 6,
                    "recovery": "recover",
                }
            }
        });

        let outcome = parse_rpc_data(peer_response).unwrap_err();
        let SocketCallError::Rpc(error) = outcome else {
            panic!("expected RPC error");
        };
        assert_eq!(error.code, crate::protocol::VERSION_SKEW_CODE);
        assert_eq!(error.message, message);
        let data = error.data.expect("typed data survives the relay");
        assert_eq!(data["client_protocol"], 2);
        assert_eq!(data["server_protocol"], 6);
        assert_eq!(data["recovery"], "recover");
    }

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
