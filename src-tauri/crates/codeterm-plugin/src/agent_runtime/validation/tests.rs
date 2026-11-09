use super::*;
use serde_json::json;

#[test]
fn context_event_wire_shape_and_owned_thread_validation() {
    let context = RuntimeContext {
        thread_id: Some("own".into()),
        ..Default::default()
    };
    let raw = json!({"kind":"context","threadId":"own","used_tokens":53000,"window_tokens":200000,"cost_micros":45000});
    let event: AgentRuntimeEvent = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(&event).unwrap(), raw);
    let reduction = RuntimeReduction {
        state: json!(null),
        events: vec![event],
    };
    assert!(validate_runtime_reduction(&context, &reduction).is_ok());
    for raw in [
        json!({"kind":"context","threadId":"foreign","used_tokens":1,"window_tokens":2}),
        json!({"kind":"context","threadId":"own","used_tokens":9007199254740992_u64,"window_tokens":2}),
        json!({"kind":"context","threadId":"own","used_tokens":1,"window_tokens":9007199254740992_u64}),
        json!({"kind":"context","threadId":"own","used_tokens":1,"window_tokens":2,"cost_micros":9007199254740992_u64}),
    ] {
        let event: AgentRuntimeEvent = serde_json::from_value(raw).unwrap();
        assert!(validate_runtime_reduction(
            &context,
            &RuntimeReduction {
                state: json!(null),
                events: vec![event]
            }
        )
        .is_err());
    }
    let zero = json!({"kind":"context","threadId":"own","used_tokens":0,"window_tokens":0});
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<AgentRuntimeEvent>(zero.clone()).unwrap())
            .unwrap(),
        zero
    );
    assert!(serde_json::from_value::<AgentRuntimeEvent>(
        json!({"kind":"context","threadId":"own","used_tokens":-1,"window_tokens":2})
    )
    .is_err());
}

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
fn only_input_offered_to_a_running_turn_can_be_declined_by_a_success_reply() {
    let capability: AgentRuntimeCapability = serde_json::from_str(include_str!(
        "../../js_runtime/parity_test/fixtures/runtime/turn-scoped-requests.json"
    ))
    .unwrap();
    let context = RuntimeContext {
        thread_id: Some("own".into()),
        ..Default::default()
    };
    let submit = |active: Option<&str>| -> RuntimeOperation {
        let mut raw = json!({"kind":"submitTurn","threadId":"own","sendId":"send","input":{"text":"opaque","attachments":[]},"settings":{}});
        if let Some(active) = active {
            raw["activeTurnId"] = json!(active);
        }
        serde_json::from_value(raw).unwrap()
    };
    let success: RpcReply = serde_json::from_value(json!({"kind":"success","value":{}})).unwrap();
    let declined: RuntimeStep = serde_json::from_value(json!({"kind":"refused","reason":{"code":"provider_refused","message":"opaque"},"beforeAcceptance":true})).unwrap();
    assert!(validate_runtime_step(
        &capability,
        &context,
        &submit(Some("turn")),
        &success,
        &declined
    )
    .is_ok());
    assert!(
        validate_runtime_step(&capability, &context, &submit(None), &success, &declined).is_err(),
        "a prompt that was answered was accepted"
    );
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

#[test]
fn append_events_preserve_item_and_thread_authority() {
    let context = RuntimeContext {
        thread_id: Some("own".into()),
        ..Default::default()
    };
    let reduction = |thread: &str, id: &str| RuntimeReduction {
        state: Value::Null,
        events: vec![AgentRuntimeEvent::ChatItemAppend {
            thread_id: thread.into(),
            item_id: id.into(),
            content: "delta".into(),
            offset: 4,
        }],
    };
    assert!(validate_runtime_reduction(&context, &reduction("own", "item")).is_ok());
    for id in ["", "bad\0id", &"x".repeat(4097)] {
        assert!(validate_runtime_reduction(&context, &reduction("own", id)).is_err());
    }
    assert!(validate_runtime_reduction(&context, &reduction("foreign", "item")).is_err());
}
