use crate::{agent_runtime::*, provider::AgentProvider};
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-opencode/tests/fixtures/runtime-contract.json"
    )))
    .unwrap()
}

async fn initialize(provider: &dyn AgentProvider, context: &RuntimeContext, fixture: &Value) {
    let initialize = RuntimeOperation::Initialize;
    let mut handshake = provider.runtime_plan(context, &initialize).await.unwrap();
    for body in [
        json!({"healthy":true,"version":"fixture"}),
        fixture["providers"].clone(),
        fixture["auth"].clone(),
    ] {
        let step = provider
            .runtime_response(
                context,
                &initialize,
                &handshake.state,
                &RpcReply::Success {
                    value: json!({"body":body,"headers":{}}),
                },
            )
            .await
            .unwrap();
        match step {
            RuntimeStep::Continue { plan, .. } => handshake = plan,
            RuntimeStep::Complete {
                result: RuntimeResult::Initialized { .. },
                ..
            } => {}
            _ => panic!("legacy handshake must complete"),
        }
    }
}

#[tokio::test]
async fn opencode_runtime_source_fixture_matches_native_quickjs_projection() {
    let fixture = fixture();
    let provider = super::opencode();
    let mut context: RuntimeContext = serde_json::from_value(fixture["context"].clone()).unwrap();
    initialize(&provider, &context, &fixture).await;
    let resume = RuntimeOperation::ResumeThread {
        thread_id: "ses_owned".into(),
        instructions: "opaque system instructions".into(),
        settings: context.settings.clone(),
    };
    let plan = provider.runtime_plan(&context, &resume).await.unwrap();
    provider
        .runtime_response(
            &context,
            &resume,
            &plan.state,
            &RpcReply::Success {
                value: json!({"body":{"id":"ses_owned","directory":"/fixture"},"headers":{}}),
            },
        )
        .await
        .unwrap();
    let submit = RuntimeOperation::SubmitTurn {
        thread_id: "ses_owned".into(),
        active_turn_id: None,
        send_id: "send-owned".into(),
        input: context.claimed_sends[0].input.clone(),
        settings: context.settings.clone(),
    };
    let plan = provider.runtime_plan(&context, &submit).await.unwrap();
    let RuntimeAction::Request { call } = &plan.action else {
        panic!("prompt must use the shared request action")
    };
    assert_eq!(call.method, "session.prompt");
    let message = call.params["body"]["messageID"].as_str().unwrap();
    let frames: Vec<RuntimeFrame> =
        serde_json::from_str(&fixture["frames"].to_string().replace("@message", message)).unwrap();
    context.stream_window = Some(RuntimeStreamWindow {
        frame_sequences: (101..101 + frames.len() as u64).collect(),
        turn_requests: vec![RuntimeTurnRequestWitness {
            send_id: "send-owned".into(),
            turn_id: message.into(),
            after_sequence: Some(100),
            written: true,
        }],
    });
    let reduction = provider
        .runtime_reduce(&context, &Value::Null, &frames)
        .await
        .unwrap();
    let events = serde_json::to_value(&reduction.events).unwrap();
    let kinds: Vec<_> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].clone())
        .collect();
    assert_eq!(json!(kinds), fixture["expectedKinds"]);
    assert_eq!(
        events
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "usage")
            .unwrap()["usage"],
        fixture["expectedUsage"]
    );
    assert_eq!(
        events
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] == "sendEvidence")
            .count(),
        1
    );
    let repeated = provider
        .runtime_reduce(&context, &reduction.state, &frames)
        .await
        .unwrap();
    assert!(!repeated
        .events
        .iter()
        .any(|event| matches!(event, AgentRuntimeEvent::SendEvidence { .. })));
    assert!(provider.runtime_plan(&context, &submit).await.is_err());
}

#[tokio::test]
async fn opencode_runtime_requests_use_the_shared_owned_reply_contract() {
    let fixture = fixture();
    let provider = super::opencode();
    let mut context: RuntimeContext = serde_json::from_value(fixture["context"].clone()).unwrap();
    initialize(&provider, &context, &fixture).await;
    for (field, choice, expected) in [
        ("permission", "once", json!({"reply":"once"})),
        ("question", "0:1", json!({"answers":[["Beta"]]})),
    ] {
        let frame: RuntimeFrame = serde_json::from_value(fixture[field].clone()).unwrap();
        let RuntimeFrame::Request {
            request_id,
            method,
            params,
        } = &frame
        else {
            panic!("request fixture required")
        };
        let reduction = provider
            .runtime_reduce(&context, &Value::Null, std::slice::from_ref(&frame))
            .await
            .unwrap();
        assert!(matches!(
            reduction.events.first(),
            Some(AgentRuntimeEvent::Prompt { .. })
        ));
        context.pending_request = Some(PendingRuntimeRequest {
            request_id: request_id.clone(),
            method: method.clone(),
            params: params.clone(),
        });
        let answer = RuntimeOperation::Answer {
            request_id: request_id.clone(),
            answer: PromptAnswer::Choice {
                values: vec![choice.into()],
            },
        };
        let plan = provider.runtime_plan(&context, &answer).await.unwrap();
        let RuntimeAction::Reply {
            result: RpcReply::Success { value },
            ..
        } = plan.action
        else {
            panic!("answer must use the shared owned reply")
        };
        assert_eq!(value, expected);
        let foreign = RuntimeOperation::Answer {
            request_id: RpcId::String("foreign".into()),
            answer: PromptAnswer::Cancel,
        };
        assert!(provider.runtime_plan(&context, &foreign).await.is_err());
    }
}

#[tokio::test]
async fn opencode_runtime_discovery_and_key_completion_use_host_login_identity() {
    let fixture = fixture();
    let provider = super::opencode();
    let context: RuntimeContext = serde_json::from_value(fixture["context"].clone()).unwrap();
    let operation = RuntimeOperation::Control {
        thread_id: Some("ses_owned".into()),
        action: "login".into(),
        value: json!({"action":"start","methodId":"api:paid","loginId":"host-challenge","apiKey":"opaque-fixture-key"}),
    };
    let mut plan = provider.runtime_plan(&context, &operation).await.unwrap();
    for body in [
        fixture["providers"].clone(),
        fixture["auth"].clone(),
        json!(true),
    ] {
        let step = provider
            .runtime_response(
                &context,
                &operation,
                &plan.state,
                &RpcReply::Success {
                    value: json!({"body":body,"headers":{}}),
                },
            )
            .await
            .unwrap();
        let RuntimeStep::Continue { plan: next, .. } = step else {
            panic!("authentication needs its discovered method and connection verification")
        };
        assert!(!next.state.to_string().contains("opaque-fixture-key"));
        plan = next;
    }
    let step = provider.runtime_response(&context, &operation, &plan.state, &RpcReply::Success {value:json!({"body":{"all":[{"id":"paid","name":"Paid Provider"}],"connected":["paid"]},"headers":{}})}).await.unwrap();
    let RuntimeStep::Complete {
        events: Some(events),
        ..
    } = step
    else {
        panic!("authentication must complete through a host-owned login event")
    };
    assert_eq!(
        serde_json::to_value(events).unwrap(),
        json!([{"kind":"authLogin","login":{"status":"completed","loginId":"host-challenge"}}])
    );
}
