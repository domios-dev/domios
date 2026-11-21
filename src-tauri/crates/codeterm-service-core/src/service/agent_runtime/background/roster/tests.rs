use super::*;
use serde_json::json;

#[test]
fn yielded_process_remains_visible_through_idle_and_clears_on_exit() {
    let context = RuntimeContext {
        thread_id: Some("root".into()),
        binding_nonce: "nonce".into(),
        epoch: 1,
        ..Default::default()
    };
    let idle = AgentRuntimeEvent::TurnState {
        thread_id: "root".into(),
        turn_id: Some("turn".into()),
        state: RuntimeTurnState::Idle,
        detail: None,
        status_line: None,
        started_at: None,
        completed_at: None,
        parent_thread_id: None,
    };
    let mut state = json!({"state":"loaded", "epoch":1});
    let roster = json!({
        "rootThreadId":"root", "bindingNonce":"nonce", "epoch":1,
        "processes":[{"threadId":"root","processId":"process","itemId":"command",
            "command":"node fixture.mjs","cwd":"/fixture","osPid":null}]
    });
    let command = |kind: &str| AgentRuntimeEvent::ChatItem {
        thread_id: "root".into(),
        item: codeterm_types::agent::chat_types::ChatMsg {
            id: "command".into(),
            kind: kind.into(),
            tool_kind: (kind == "tool_call").then(|| "command".into()),
            ..Default::default()
        },
    };
    assert!(changed_by(&command("tool_result")));
    assert!(replace_projection(
        &mut state,
        &context,
        Some(roster.clone())
    ));
    assert!(changed_by(&idle));
    let mut idle_state = json!({"state":"loaded", "epoch":1,"last_turn_duration_ms":10});
    preserve_projection(&state, &mut idle_state, &context);
    assert_eq!(idle_state[FIELD], roster);
    assert!(!replace_projection(&mut idle_state, &context, Some(roster)));
    assert!(changed_by(&command("tool_call")));
    assert!(replace_projection(&mut idle_state, &context, None));
    assert!(idle_state.get(FIELD).is_none());

    let mut replacement = json!({"state":"loaded", "epoch":2});
    preserve_projection(
        &state,
        &mut replacement,
        &RuntimeContext {
            epoch: 2,
            ..context
        },
    );
    assert!(replacement.get(FIELD).is_none());
}
