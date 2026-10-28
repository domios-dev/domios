use super::*;
use crate::client::{RpcClient, RpcError, SocketCallError};
use serde_json::{json, Value};

struct Preflight {
    denied: bool,
    calls: usize,
}
impl RpcClient for Preflight {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls += 1;
        assert_eq!(method, "mem.secret.authorize");
        assert_eq!(params, Some(json!({})));
        if self.denied {
            Err(SocketCallError::Rpc(RpcError {
                code: -32003,
                message: "neighbor_capability_denied: This Neighbor operation is not allowed"
                    .into(),
                data: Some(json!({"reason":"neighbor_capability_denied"})),
            }))
        } else {
            Ok(json!({"authorized":true}))
        }
    }
}

#[test]
fn secret_preflight_keeps_the_authenticated_neighbor_denial_before_backend_work() {
    let mut rpc = Preflight {
        denied: true,
        calls: 0,
    };
    let mut backend_calls = 0;
    let result = authorize_secrets_rpc(&mut rpc).map(|()| {
        backend_calls += 1;
    });
    let error = result.unwrap_err();
    assert_eq!(backend_calls, 0);
    assert_eq!(rpc.calls, 1);
    let (status, body) = crate::cmd::mem_secret_failure::transport_failure(&error);
    let (exit, error) = crate::cmd::mem_secret_failure::secret_failure(status, &body).unwrap();
    assert_eq!(exit, 1);
    assert_eq!(error.code, Some(-32003));
    let value =
        crate::cmd::error_contract::error_envelope(&error.message, error.code, error.data.as_ref());
    assert_eq!(value["error"]["reason"], "neighbor_capability_denied");
    assert_eq!(value["error"]["code"], "refused");
    assert_eq!(value["error"]["rpc_code"], -32003);
}

#[test]
fn ordinary_secret_preflight_confirms_access_without_fetching_any_secret() {
    let mut rpc = Preflight {
        denied: false,
        calls: 0,
    };
    assert!(authorize_secrets_rpc(&mut rpc).is_ok());
    assert_eq!(rpc.calls, 1);
}
