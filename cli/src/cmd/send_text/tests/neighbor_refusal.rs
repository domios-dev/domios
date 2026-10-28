use crate::client::{RpcClient, RpcError, SocketCallError};
use serde_json::{json, Value};

struct Denied;

impl RpcClient for Denied {
    fn call_detailed(&mut self, method: &str, _: Option<Value>) -> Result<Value, SocketCallError> {
        assert_eq!(method, "tab.list");
        Err(SocketCallError::Rpc(RpcError {
            code: -32003,
            message: "neighbor_capability_denied: This concierge cannot enumerate tabs".into(),
            data: Some(
                json!({"reason":"neighbor_capability_denied","tabs":[{"id":"foreign-private-tab","title":"foreign-private-title"}]}),
            ),
        }))
    }
}

#[test]
fn denied_send_preflight_preserves_neighbor_refusal_instead_of_unknown_target() {
    let error = super::super::probe_tab_kind(&mut Denied, "target").unwrap_err();
    assert_eq!(
        error.message,
        "neighbor_capability_denied: This concierge cannot enumerate tabs"
    );
    let mut out = Vec::new();
    let mut err = Vec::new();
    crate::cmd::print::write_cli_error(true, &error, &mut out, &mut err);
    let response: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(response["error"]["code"], "refused");
    assert_eq!(response["error"]["reason"], "neighbor_capability_denied");
    assert_eq!(response["error"]["rpc_code"], -32003);
    assert!(!response.to_string().contains("foreign-private"));
    assert!(!response.to_string().contains("could not confirm"));
    assert!(err.is_empty());
}
