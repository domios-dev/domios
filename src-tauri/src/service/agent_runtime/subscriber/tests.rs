use super::*;
use crate::service::agent_runtime::model::RuntimeState;
use serde_json::{json, Value};

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
