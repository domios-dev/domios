use super::*;

#[test]
fn mesh_execution_is_refused_before_the_remote_can_claim_local_authority() {
    struct Remote;
    impl RpcClient for Remote {
        fn is_remote(&self) -> bool {
            true
        }
        fn call_detailed(&mut self, _: &str, _: Option<Value>) -> Result<Value, SocketCallError> {
            panic!("origin operations never relay as remote RPC")
        }
    }
    let error = call(
        &mut Remote,
        "team.adopt_agent",
        json!({"peer":"worker-peer"}),
    )
    .unwrap_err();
    assert!(error.to_string().contains("use --peer instead of --mesh"));
}
