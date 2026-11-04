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
