use crate::{agent_runtime::*, provider::AgentProvider};
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/runtime-http-sse.json"
    )))
    .unwrap()
}

async fn response(
    provider: &dyn AgentProvider,
    context: &RuntimeContext,
    operation: &RuntimeOperation,
    state: &Value,
    body: Value,
) -> RuntimeStep {
    let reply = RpcReply::Success {
        value: json!({"body":body,"headers":{}}),
    };
    let step = provider
        .runtime_response(context, operation, state, &reply)
        .await
        .unwrap();
    validate_runtime_step(
        &provider.agent_runtime().unwrap(),
        context,
        operation,
        &reply,
        &step,
    )
    .unwrap();
    step
}

async fn initialize(provider: &dyn AgentProvider, context: &RuntimeContext, fixture: &Value) {
    let operation = RuntimeOperation::Initialize;
    let mut plan = provider.runtime_plan(context, &operation).await.unwrap();
    for field in ["health", "providers", "integrations"] {
        let step = response(
            provider,
            context,
            &operation,
            &plan.state,
            fixture[field].clone(),
        )
        .await;
        match step {
            RuntimeStep::Continue { plan: next, .. } => plan = next,
            RuntimeStep::Complete {
                result: RuntimeResult::Initialized { .. },
                ..
            } => assert_eq!(field, "integrations"),
            _ => panic!("initialization must verify the selected protocol and catalogue"),
        }
    }
}

async fn history_and_active(
    provider: &dyn AgentProvider,
    context: &RuntimeContext,
    body: Value,
    active: Value,
) -> (
    Vec<codeterm_types::agent::chat_types::ChatMsg>,
    Vec<AgentRuntimeEvent>,
) {
    let operation = RuntimeOperation::ReadHistory {
        thread_id: context.thread_id.clone().unwrap(),
        cursor: None,
        order: Some(HistoryOrder::Desc),
    };
    let plan = provider.runtime_plan(context, &operation).await.unwrap();
    let RuntimeStep::Continue {
        plan,
        history: Some(RuntimeResult::HistoryPage { items, .. }),
        ..
    } = response(provider, context, &operation, &plan.state, body).await
    else {
        panic!("history chunk and status request required");
    };
    let RuntimeAction::Request { call } = &plan.action else {
        panic!("active sessions request required");
    };
    assert_eq!(call.method, "v2.session.active");
    let RuntimeStep::Complete {
        result: RuntimeResult::HistoryPage { .. },
        events,
        ..
    } = response(provider, context, &operation, &plan.state, active).await
    else {
        panic!("history completion required");
    };
    (items, events.unwrap_or_default())
}

#[tokio::test]
async fn opencode_v2_recorded_tool_permission_text_and_history_match_quickjs() {
    let fixture = fixture();
    let provider = super::opencode();
    let mut context: RuntimeContext = serde_json::from_value(fixture["context"].clone()).unwrap();
    initialize(&provider, &context, &fixture).await;
    let resume = RuntimeOperation::ResumeThread {
        thread_id: "ses_owned".into(),
        instructions: "opaque instructions".into(),
        settings: context.settings.clone(),
    };
    let mut plan = provider.runtime_plan(&context, &resume).await.unwrap();
    for body in [fixture["session"].clone(), Value::Null, Value::Null] {
        let step = response(&provider, &context, &resume, &plan.state, body).await;
        match step {
            RuntimeStep::Continue { plan: next, .. } => plan = next,
            RuntimeStep::Complete {
                result: RuntimeResult::ThreadLoaded { thread_id, .. },
                ..
            } => assert_eq!(thread_id, "ses_owned"),
            _ => {
                panic!("resume must verify the exact session and reapply settings and instructions")
            }
        }
    }
    let submit = RuntimeOperation::SubmitTurn {
        thread_id: "ses_owned".into(),
        active_turn_id: None,
        send_id: "send-owned".into(),
        input: context.claimed_sends[0].input.clone(),
        settings: context.settings.clone(),
    };
    let plan = provider.runtime_plan(&context, &submit).await.unwrap();
    let RuntimeAction::Request { call } = &plan.action else {
        panic!("request required")
    };
    assert_eq!(call.method, "v2.session.prompt");
    let message = call.params["body"]["id"].as_str().unwrap();
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
    validate_runtime_reduction(&context, &reduction).unwrap();
    let events = serde_json::to_value(&reduction.events).unwrap();
    let events = events.as_array().unwrap();
    for (kind, count) in [("sendEvidence", 1), ("usage", 2), ("prompt", 2)] {
        assert_eq!(
            events.iter().filter(|event| event["kind"] == kind).count(),
            count,
            "{kind}"
        );
    }
    assert!(events
        .iter()
        .any(|event| event["item"]["toolName"] == "bash"
            && event["item"]["toolStatus"] == "completed"));
    assert!(events
        .iter()
        .any(|event| event["item"]["content"] == "Fixture tool turn complete."));
    assert!(events
        .iter()
        .filter(|event| matches!(
            event["item"]["type"].as_str(),
            Some("assistant" | "thinking")
        ))
        .all(|event| event["kind"] == "chatItem"));
    assert!(provider
        .runtime_reduce(&context, &reduction.state, &frames)
        .await
        .unwrap()
        .events
        .is_empty());
    let receipt: Value =
        serde_json::from_str(&fixture["receipt"].to_string().replace("@message", message)).unwrap();
    let reply = response(&provider, &context, &submit, &plan.state, receipt).await;
    assert!(matches!(
        reply,
        RuntimeStep::Complete {
            result: RuntimeResult::TurnAccepted { .. },
            ..
        }
    ));
    context.settings.effort = Some("default".into());
    context.claimed_sends[0].send_id = "second-send".into();
    let second = RuntimeOperation::SubmitTurn {
        thread_id: "ses_owned".into(),
        active_turn_id: None,
        send_id: "second-send".into(),
        input: context.claimed_sends[0].input.clone(),
        settings: context.settings.clone(),
    };
    let RuntimeAction::Request { call } = provider
        .runtime_plan(&context, &second)
        .await
        .unwrap()
        .action
    else {
        panic!("second default-effort turn must send a plain prompt")
    };
    assert_eq!(call.method, "v2.session.prompt");
    assert_eq!(call.params["path"], json!({"sessionID":"ses_owned"}));
    assert_eq!(
        call.params["body"]["text"],
        context.claimed_sends[0].input.text
    );
    assert!(call.params["body"].get("model").is_none());
    for frame in frames
        .iter()
        .filter(|frame| matches!(frame, RuntimeFrame::Request { .. }))
    {
        let RuntimeFrame::Request {
            request_id,
            method,
            params,
        } = frame
        else {
            unreachable!()
        };
        context.pending_request = Some(PendingRuntimeRequest {
            request_id: request_id.clone(),
            method: method.clone(),
            params: params.clone(),
        });
        let permission = method == "permission.asked";
        let answer = RuntimeOperation::Answer {
            request_id: request_id.clone(),
            answer: PromptAnswer::Choice {
                values: vec![if permission { "once" } else { "0:1" }.into()],
            },
        };
        let plan = provider.runtime_plan(&context, &answer).await.unwrap();
        let RuntimeAction::Reply {
            result: RpcReply::Success { value },
            ..
        } = plan.action
        else {
            panic!("owned reply required")
        };
        assert_eq!(
            value,
            if permission {
                json!({"decision":"once"})
            } else {
                json!({"answer":{"choice":"beta"}})
            }
        );
    }
    let (items, _) = history_and_active(
        &provider,
        &context,
        fixture["history"].clone(),
        json!({"data": {}}),
    )
    .await;
    assert_eq!(items.first().unwrap().id, "@message");
    assert_eq!(items.last().unwrap().content, "Fixture tool turn complete.");
}

#[tokio::test]
async fn opencode_v2_recorded_same_ordinal_fragments_match_resumed_history() {
    let fixture = fixture();
    let provider = super::opencode();
    let mut context: RuntimeContext = serde_json::from_value(fixture["context"].clone()).unwrap();
    initialize(&provider, &context, &fixture).await;
    context.active_turn_id = Some("resumed-turn".into());
    let capture = &fixture["sameMessageFragments"];
    let frames: Vec<RuntimeFrame> = serde_json::from_value(capture["frames"].clone()).unwrap();
    let mut state = Value::Null;
    let mut events = Vec::new();
    for frame in frames {
        let reduction = provider
            .runtime_reduce(&context, &state, &[frame])
            .await
            .unwrap();
        validate_runtime_reduction(&context, &reduction).unwrap();
        state = reduction.state;
        events.extend(reduction.events);
    }
    let events = serde_json::to_value(events).unwrap();
    let events = events.as_array().unwrap();
    assert_eq!(events.last().unwrap()["state"], "idle");
    let live: Vec<_> = events
        .iter()
        .filter(|event| event["kind"] == "chatItem" && event["item"]["content"] != "")
        .map(|event| &event["item"])
        .collect();
    assert_eq!(live.len(), 2);
    assert_eq!(live[0]["content"], "Fixture reasoning.");
    assert_eq!(live[1]["content"], "Fixture final.");
    assert_ne!(live[0]["id"], live[1]["id"]);

    let (items, _) = history_and_active(
        &provider,
        &context,
        capture["history"].clone(),
        json!({"data": {}}),
    )
    .await;
    assert_eq!(items.len(), 3);
    for (row, streamed) in items.iter().skip(1).zip(live) {
        assert_eq!(streamed["id"], row.id);
        assert_eq!(streamed["type"], row.kind);
        assert_eq!(streamed["content"], row.content);
    }
}

#[tokio::test]
async fn opencode_v2_restore_recovers_an_interrupted_tool_without_the_previous_reducer() {
    let fixture = fixture();
    let capture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/opencode-v2-abort.json"
    )))
    .unwrap();
    let provider = super::opencode();
    let context: RuntimeContext = serde_json::from_value(fixture["context"].clone()).unwrap();
    initialize(&provider, &context, &fixture).await;
    let frames: Vec<RuntimeFrame> = serde_json::from_value(capture["frames"].clone()).unwrap();
    let reduction = provider
        .runtime_reduce(&context, &Value::Null, &frames)
        .await
        .unwrap();
    validate_runtime_reduction(&context, &reduction).unwrap();
    assert!(reduction.events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ChatItem { item, .. } if item.id == "msg_restore:tool:call_restore"
            && item.tool_completed_at_ms == Some(1795071728671)
    )));
    let (items, events) = history_and_active(
        &provider,
        &context,
        capture["history"].clone(),
        json!({"data": {"ses_foreign": {"type": "running"}}}),
    )
    .await;
    assert_eq!(items.last().unwrap().id, "msg_restore:tool:call_restore");
    assert!(matches!(
        &events[..],
        [AgentRuntimeEvent::TurnState {
            state: RuntimeTurnState::Idle,
            detail: Some(RuntimeTurnDetail::Interrupted),
            ..
        }]
    ));
}
