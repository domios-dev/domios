use super::*;
use serde_json::json;

#[test]
fn turn_scoped_request_requires_exact_owned_target_and_valid_turn_id() {
    let mut capability: AgentRuntimeCapability = serde_json::from_str(include_str!(
        "../../js_runtime/parity_test/fixtures/runtime/turn-scoped-requests.json"
    ))
    .unwrap();
    let context = RuntimeContext {
        thread_id: Some("own".into()),
        ..Default::default()
    };
    let operation:RuntimeOperation=serde_json::from_value(json!({"kind":"submitTurn","threadId":"own","sendId":"send","input":{"text":"opaque","attachments":[]},"settings":{}})).unwrap();
    let mut call:RpcCall=serde_json::from_value(json!({"method":"opaque/prompt","params":{},"target":{"threadId":"own","turnId":"new-turn"}})).unwrap();
    assert!(validate_runtime_call(&capability, &context, &operation, &call).is_ok());
    for turn in [String::new(), "contains\0nul".into(), "x".repeat(4097)] {
        call.target.as_mut().unwrap().turn_id = Some(turn);
        assert!(validate_runtime_call(&capability, &context, &operation, &call).is_err());
    }
    call.target.as_mut().unwrap().turn_id = Some("new-turn".into());
    call.target.as_mut().unwrap().thread_id = "foreign".into();
    assert!(validate_runtime_call(&capability, &context, &operation, &call).is_err());
    call.target.as_mut().unwrap().thread_id = "own".into();
    capability.turn_scoped_requests.clear();
    assert!(validate_runtime_call(&capability, &context, &operation, &call).is_err());
    call.target = None;
    assert!(validate_runtime_call(&capability, &context, &operation, &call).is_ok());
    capability.turn_scoped_requests = vec!["submitTurn".into()];
    assert!(validate_runtime_call(&capability, &context, &operation, &call).is_err());
}

#[test]
fn stream_window_refuses_malformed_ids_and_sequence_boundaries() {
    let base = serde_json::json!({"epoch":1,"tabId":"tab","bindingNonce":"nonce","cwd":"/work","hostVersion":"1.0.0","claimedSends":[],"settings":{},"providerSettings":{},
        "streamWindow":{"frameSequences":[11],"turnRequests":[{"sendId":"send","turnId":"turn","afterSequence":10,"written":true}]}});
    let ctx: RuntimeContext = serde_json::from_value(base.clone()).unwrap();
    assert!(validate_runtime_context(&ctx).is_ok());
    for window in [
        serde_json::json!({"frameSequences":[11,11]}),
        serde_json::json!({"frameSequences":[0]}),
        serde_json::json!({"frameSequences":[9007199254740992_u64]}),
        serde_json::json!({"turnRequests":[{"sendId":"send","turnId":"","afterSequence":10,"written":true}]}),
        serde_json::json!({"turnRequests":[{"sendId":"send","turnId":"turn","written":true}]}),
    ] {
        let mut invalid = base.clone();
        invalid["streamWindow"] = window;
        let ctx: RuntimeContext = serde_json::from_value(invalid).unwrap();
        assert!(validate_runtime_context(&ctx).is_err());
    }
}
