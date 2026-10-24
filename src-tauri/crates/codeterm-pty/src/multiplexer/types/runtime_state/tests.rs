use super::*;
#[test]
fn login_values_expire_in_live_tab_serialization() {
    let state = Some(
        serde_json::json!({"state":"loginPending","login":{"status":"pending","url":"https://example.invalid/opaque-path","userCode":"opaque-token","expiresAtMs":5000}}),
    );
    let visible = project(state.clone(), 4999).unwrap();
    assert_eq!(visible["login"]["userCode"], "opaque-token");
    let hidden = project(state, 5000).unwrap();
    assert!(hidden["login"].get("url").is_none());
    assert!(hidden["login"].get("userCode").is_none());
    assert_eq!(hidden["login"]["status"], "failed");
}

#[test]
fn expired_login_values_are_removed_from_actual_tab_readback() {
    let (mut mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().agent_runtime_state = Some(
        serde_json::json!({"state":"loginPending","login":{"status":"pending","url":"https://example.invalid/opaque-path","userCode":"opaque-token","expiresAtMs":0}}),
    );
    let live = serde_json::to_value(mux.get_tab(&tab).unwrap()).unwrap();
    assert_eq!(live["agent_runtime_state"]["login"]["status"], "failed");
    assert!(live["agent_runtime_state"]["login"].get("url").is_none());
    assert!(live["agent_runtime_state"]["login"]
        .get("userCode")
        .is_none());
}
