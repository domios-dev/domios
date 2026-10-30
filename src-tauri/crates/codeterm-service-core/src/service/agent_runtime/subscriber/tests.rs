use super::*;
use crate::service::agent_runtime::model::RuntimeState;
use serde_json::{json, Value};

#[test]
fn crash_classification_precedes_requested_cleanup_exit() {
    assert!(unexpected_exit(None));
    assert!(unexpected_exit(Some(&RuntimeFailure::PipeEof)));
    assert!(unexpected_exit(Some(&RuntimeFailure::ProcessExited {
        code: Some(1),
        signal: None,
        stderr: "crash".into(),
        requested: false,
    })));
    assert!(!unexpected_exit(Some(&RuntimeFailure::ProcessExited {
        code: Some(0),
        signal: None,
        stderr: String::new(),
        requested: true,
    })));
    assert!(!unexpected_exit(Some(&RuntimeFailure::Superseded)));
}

#[test]
fn teardown_preserves_degraded_reason_and_refuses_old_epoch() {
    let mut state = RuntimeState::unloaded(Some("own".into()));
    let epoch = state.begin();
    state.load(epoch, "own");
    let failure = RuntimeFailure::HookFailed("durable status write failed".into());
    state.degrade(epoch, &failure);
    let reason = state.snapshot().reason;
    assert!(!unload_retaining_degradation(
        &mut state,
        epoch + 1,
        &RuntimeFailure::PipeEof
    ));
    assert!(unload_retaining_degradation(
        &mut state,
        epoch,
        &RuntimeFailure::PipeEof
    ));
    assert!(matches!(state.phase, RuntimePhase::Degraded { .. }));
    assert_eq!(state.snapshot().reason, reason);
}

#[test]
fn ordinary_pipe_exit_still_unloads_without_fabricating_a_degraded_state() {
    let mut state = RuntimeState::unloaded(Some("own".into()));
    let epoch = state.begin();
    state.load(epoch, "own");
    assert!(unload_retaining_degradation(
        &mut state,
        epoch,
        &RuntimeFailure::PipeEof
    ));
    assert!(matches!(state.phase, RuntimePhase::Unloaded { .. }));
    assert!(!unload_retaining_degradation(
        &mut state,
        epoch,
        &RuntimeFailure::PipeEof
    ));
}

#[test]
fn outstanding_request_bound_refuses_at_capacity() {
    assert!(!outstanding_bound_exceeded(0));
    assert!(!outstanding_bound_exceeded(MAX_OUTSTANDING_REQUESTS - 1));
    assert!(outstanding_bound_exceeded(MAX_OUTSTANDING_REQUESTS));
    assert!(outstanding_bound_exceeded(usize::MAX));
}

#[test]
fn unmapped_requests_get_correlated_error_decision() {
    let context = RuntimeContext {
        thread_id: Some("own".into()),
        ..Default::default()
    };
    let id = RpcId::String("request".into());
    assert_eq!(
        unmapped_request_error(&context, &id, &[]).unwrap().code,
        -32601
    );
    let prompt = codeterm_types::pty::PendingPrompt {
        request_id: None,
        question: "opaque".into(),
        options: Vec::new(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    let own = AgentRuntimeEvent::Prompt {
        thread_id: "own".into(),
        request_id: id.clone(),
        prompt: prompt.clone(),
    };
    assert!(unmapped_request_error(&context, &id, &[own]).is_none());
    let foreign = AgentRuntimeEvent::Prompt {
        thread_id: "foreign".into(),
        request_id: id.clone(),
        prompt: prompt.clone(),
    };
    assert!(unmapped_request_error(&context, &id, &[foreign]).is_some());
    let unrelated = AgentRuntimeEvent::Prompt {
        thread_id: "own".into(),
        request_id: RpcId::Number(4),
        prompt,
    };
    assert!(unmapped_request_error(&context, &id, &[unrelated]).is_some());
}

#[tokio::test]
async fn mock_runtime_unmapped_requests_reply_and_capacity_stays_bounded() {
    let mut scene = fixtures::scene();
    let process = scene.session.process.lock().clone().unwrap();
    start(
        scene.fixture.service.clone(),
        scene.session.provider.clone(),
        scene.session.clone(),
        process.clone(),
    );
    for (id, method, params) in [
        (json!("unknown"), "fixture/unknown", json!({})),
        (json!(8), "fixture/known", json!({"threadId":"foreign"})),
        (json!(9), "fixture/known", json!({"malformed":true})),
    ] {
        fixtures::write(
            &mut scene.peer,
            json!({"id":id,"method":method,"params":params}),
        )
        .await;
        let reply = fixtures::read(&mut scene.peer).await;
        assert_eq!(reply["id"], id);
        assert_eq!(reply["error"]["code"], -32601);
        assert!(scene.session.prompts.lock().is_empty());
    }
    {
        let mut pending = scene.session.prompts.lock();
        for id in 0..MAX_OUTSTANDING_REQUESTS {
            let request_id = RpcId::Number(id as i64);
            pending.insert(
                request_id.clone(),
                engine::OutstandingRequest {
                    request: PendingRuntimeRequest {
                        request_id,
                        method: "opaque".into(),
                        params: Value::Null,
                    },
                    prompt: None,
                },
            );
        }
    }
    fixtures::write(
        &mut scene.peer,
        json!({"id":"over-bound","method":"fixture/known","params":{}}),
    )
    .await;
    let reply = fixtures::read(&mut scene.peer).await;
    assert_eq!(reply["id"], "over-bound");
    assert_eq!(reply["error"]["code"], -32601);
    assert_eq!(scene.session.prompts.lock().len(), MAX_OUTSTANDING_REQUESTS);
    process.client.close(RuntimeFailure::Superseded);
}

#[test]
fn live_trace_origin_requires_current_own_turn_and_never_contains_body() {
    use crate::service::tab_chat::structured_sink::live_runtime_origin;
    let before = RuntimeContext {
        tab_id: "opaque-live-tab".into(),
        epoch: 7,
        binding_nonce: "opaque-nonce".into(),
        thread_id: Some("opaque-thread".into()),
        active_turn_id: Some("opaque-turn".into()),
        ..Default::default()
    };
    let item = crate::agent::chat_types::ChatMsg {
        id: "opaque-item".into(),
        kind: "assistant".into(),
        content: "opaque-private-body".into(),
        tool_flow_id: Some("opaque-turn".into()),
        ..Default::default()
    };
    let origin = live_runtime_origin(true, &before, &before, "opaque-thread", &item).unwrap();
    assert_eq!(
        serde_json::to_value(origin).unwrap(),
        json!({"kind":"runtime_live","turn":"opaque-turn"})
    );
    assert!(live_runtime_origin(false, &before, &before, "opaque-thread", &item).is_none(),
        "default runtimeResponse/control/goal publication cannot mark even an exact active assistant live");
    for current in [
        RuntimeContext {
            tab_id: "foreign".into(),
            ..before.clone()
        },
        RuntimeContext {
            epoch: 8,
            ..before.clone()
        },
        RuntimeContext {
            epoch: 0,
            ..before.clone()
        },
        RuntimeContext {
            binding_nonce: "foreign".into(),
            ..before.clone()
        },
        RuntimeContext {
            thread_id: Some("foreign".into()),
            ..before.clone()
        },
        RuntimeContext {
            active_turn_id: None,
            ..before.clone()
        },
        RuntimeContext {
            active_turn_id: Some("stale-turn".into()),
            ..before.clone()
        },
    ] {
        assert!(live_runtime_origin(true, &before, &current, "opaque-thread", &item).is_none());
    }
    assert!(live_runtime_origin(true, &before, &before, "foreign", &item).is_none());
    for stale in [
        crate::agent::chat_types::ChatMsg {
            tool_flow_id: None,
            ..item.clone()
        },
        crate::agent::chat_types::ChatMsg {
            tool_flow_id: Some("stale-turn".into()),
            ..item.clone()
        },
        crate::agent::chat_types::ChatMsg {
            inherited_from_session_id: Some("parent".into()),
            ..item.clone()
        },
    ] {
        assert!(live_runtime_origin(true, &before, &before, "opaque-thread", &stale).is_none());
    }
}

#[test]
fn cached_assistant_emit_has_no_live_diagnostic_provenance() {
    let ui = crate::service::test_fixture::RecordedUiEvents::default();
    let cached = crate::agent::chat_types::ChatMsg {
        id: "opaque-cached-assistant".into(),
        kind: "assistant".into(),
        tool_flow_id: Some("opaque-current-turn".into()),
        ..Default::default()
    };
    crate::service::tab_chat::structured_sink::emit_chat_msgs(
        &ui,
        "opaque-cache-trace-tab",
        &[cached],
        None,
        Some("opaque-thread"),
        Some("opaque-nonce"),
    );
    let events = ui.0.lock();
    assert_eq!(events.len(), 1);
    assert!(events[0].1.get("diagnostic_live").is_none());
    assert_eq!(events[0].1["messages"][0]["id"], "opaque-cached-assistant");
}
