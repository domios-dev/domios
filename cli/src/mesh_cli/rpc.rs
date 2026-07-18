use crate::client::{RpcClient, SocketCallError};

pub struct MeshRpcClient {
    http: reqwest::blocking::Client,
    peer_url: String,
    signed_token: String,
}

impl MeshRpcClient {
    pub fn connect(base: &str, token: &Option<String>, target: &str) -> Result<Self, String> {
        let http =
            crate::build_http(token).map_err(|e| format!("failed to build HTTP client: {e}"))?;
        let peer_url = super::relay::resolve_peer_url(&http, base, token, target)
            .ok_or_else(|| format!("peer '{target}' not found or has no reachable address"))?;
        let signed_token = crate::read_mesh_signed_token().ok_or_else(|| {
            "cannot sign mesh request: local mesh identity is unavailable".to_string()
        })?;
        Ok(Self {
            http,
            peer_url,
            signed_token,
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
        let response = super::relay::relay_call(
            &self.http,
            &self.peer_url,
            &self.signed_token,
            "POST",
            "/api/mesh/rpc",
            Some(&body.to_string()),
        )
        .map_err(SocketCallError::Transport)?;
        parse_relay_response(response)
    }

    fn is_remote(&self) -> bool {
        true
    }
}

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
