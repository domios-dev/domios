use super::*;
#[test]
fn exit_unloads_only_the_own_epoch_and_resume_cannot_change_threads() {
    let mut state = RuntimeState::unloaded(Some("own".into()));
    let epoch = state.begin();
    assert!(!state.load(epoch, "foreign"));
    assert!(state.load(epoch, "own"));
    assert!(!state.unload(epoch + 1, &RuntimeFailure::PipeEof));
    assert!(state.unload(epoch, &RuntimeFailure::PipeEof));
    assert!(!state.unload(epoch, &RuntimeFailure::PipeEof));
    let next = state.begin();
    assert!(state.load(next, "own"));
    assert_eq!(state.thread_id.as_deref(), Some("own"));
}
#[test]
fn degraded_reason_remains_until_an_explicit_new_attempt() {
    let mut state = RuntimeState::unloaded(None);
    let epoch = state.begin();
    assert!(state.degrade(epoch, &RuntimeFailure::PipeEof));
    assert_eq!(
        serde_json::to_value(&state).unwrap()["phase"]["kind"],
        "degraded"
    );
    assert!(!state.degrade(epoch + 1, &RuntimeFailure::Deadline));
    assert!(matches!(state.phase, RuntimePhase::Degraded { .. }));
    state.begin();
    assert_eq!(state.phase, RuntimePhase::Loading);
}
#[test]
fn history_and_live_items_reconcile_without_duplicate_or_changed_order() {
    let mut cache = ItemCache::default();
    for (id, text) in [("a", "partial"), ("b", "tool"), ("a", "completed")] {
        cache.reconcile(ChatMsg {
            id: id.into(),
            content: text.into(),
            ..Default::default()
        });
    }
    let items = cache.snapshot();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "a");
    assert_eq!(items[0].content, "completed");
    assert_eq!(items[0].seq, 0);
    assert_eq!(items[1].seq, 1);
}

#[test]
fn snapshot_is_the_public_runtime_state_with_an_honest_failure_reason() {
    let mut state = RuntimeState::unloaded(Some("thread".into()));
    assert_eq!(
        serde_json::to_value(state.snapshot()).unwrap(),
        serde_json::json!({"state":"unloaded","epoch":0})
    );
    let epoch = state.begin();
    assert_eq!(state.snapshot().state, "loading");
    assert!(state.load(epoch, "thread"));
    assert_eq!(state.snapshot().state, "loaded");
    state.degrade(epoch, &RuntimeFailure::PipeEof);
    let snapshot = state.snapshot();
    assert_eq!(snapshot.state, "degraded");
    assert!(snapshot.reason.is_some());
}

#[test]
fn live_evidence_requires_the_exact_claimed_input_and_thread_in_the_same_reduction() {
    use codeterm_plugin::agent_runtime::{AgentRuntimeEvent, ClaimedSend, TurnInput};
    let claim = ClaimedSend {
        send_id: "send".into(),
        thread_id: "own".into(),
        input: TurnInput {
            text: "opaque input".into(),
            attachments: Vec::new(),
        },
    };
    let evidence = AgentRuntimeEvent::SendEvidence {
        thread_id: "own".into(),
        send_id: "send".into(),
        turn_id: "turn".into(),
    };
    assert!(validated_evidence(&[claim.clone()], &[evidence.clone()]).is_empty());
    let mut row = ChatMsg {
        kind: "user".into(),
        content: "other input".into(),
        ..Default::default()
    };
    let event = |row| AgentRuntimeEvent::ChatItem {
        thread_id: "own".into(),
        item: row,
    };
    assert!(
        validated_evidence(&[claim.clone()], &[event(row.clone()), evidence.clone()]).is_empty()
    );
    row.content = claim.input.text.clone();
    assert!(
        validated_evidence(&[claim.clone()], &[event(row.clone()), evidence.clone()])
            .contains("send")
    );
    let mut foreign = claim;
    foreign.thread_id = "foreign".into();
    assert!(validated_evidence(&[foreign], &[event(row), evidence]).is_empty());
}

#[test]
fn provider_context_excludes_credentials_but_retains_nonsecret_settings() {
    let config = serde_json::json!({"model":"m", "apiToken":"fixture", "nested":{"api_key":"fixture","effort":"high"}});
    assert_eq!(
        public_provider_settings(&config),
        serde_json::json!({"model":"m","nested":{"effort":"high"}})
    );
}

#[test]
fn prompt_answers_cannot_select_a_foreign_option_or_duplicate_values() {
    use codeterm_plugin::agent_runtime::PromptAnswer;
    let prompt = codeterm_types::pty::PendingPrompt {
        request_id: None,
        question: "opaque".into(),
        options: vec![codeterm_types::pty::PromptOption {
            label: "opaque".into(),
            value: "own".into(),
        }],
        sections: None,
        multi_select: Some(true),
        submit_available: None,
        selected_values: None,
    };
    assert!(validate_prompt_answer(
        &prompt,
        &PromptAnswer::Choice {
            values: vec!["foreign".into()]
        }
    )
    .is_err());
    assert!(validate_prompt_answer(
        &prompt,
        &PromptAnswer::Choice {
            values: vec!["own".into(), "own".into()]
        }
    )
    .is_err());
    assert!(validate_prompt_answer(
        &prompt,
        &PromptAnswer::Choice {
            values: vec!["own".into()]
        }
    )
    .is_ok());
}

#[test]
fn timing_survives_snapshot_restore_and_completion_is_recorded_once() {
    let mut state = RuntimeState::unloaded(Some("own".into()));
    state.begin();
    state.working(false, 1000, Some(900));
    state.working(true, 1500, None);
    assert_eq!(state.active_turn_started_at_ms, Some(900));
    let saved = serde_json::to_value(state.snapshot()).unwrap();
    let mut restored = RuntimeState::unloaded(Some("own".into()));
    restored.restore_timing(Some(&saved));
    assert_eq!(restored.begin(), 2);
    restored.completed(3000, Some(2900));
    assert_eq!(restored.last_turn_duration_ms, Some(2000));
    restored.completed(4000, None);
    assert_eq!(restored.last_turn_duration_ms, Some(2000));
}

#[test]
fn autonomous_following_turn_is_allowed_and_old_completion_cannot_stop_it() {
    use codeterm_plugin::agent_runtime::RuntimeTurnState;
    assert!(accepts_turn_transition(
        Some("first"),
        Some("following"),
        &RuntimeTurnState::Working
    ));
    assert!(!accepts_turn_transition(
        Some("following"),
        Some("first"),
        &RuntimeTurnState::Idle
    ));
    assert!(accepts_turn_transition(
        Some("following"),
        Some("following"),
        &RuntimeTurnState::Idle
    ));
}

#[test]
fn completed_parent_retains_ancestry_and_descendant_interrupt_authority() {
    use codeterm_plugin::agent_runtime::*;
    let mut context = RuntimeContext {
        thread_id: Some("root".into()),
        ..Default::default()
    };
    let event =
        |thread: &str, turn: &str, parent: Option<&str>, state| AgentRuntimeEvent::TurnState {
            thread_id: thread.into(),
            turn_id: Some(turn.into()),
            parent_thread_id: parent.map(str::to_string),
            state,
            started_at: None,
            completed_at: None,
        };
    for event in [
        event("child", "a", Some("root"), RuntimeTurnState::Working),
        event("grandchild", "b", Some("child"), RuntimeTurnState::Working),
        event("child", "a", Some("root"), RuntimeTurnState::Idle),
    ] {
        observe_turn(&mut context, &event);
    }
    assert_eq!(context.thread_parents.len(), 2);
    assert_eq!(context.active_turns.len(), 1);
    assert_eq!(context.active_turns[0].thread_id, "grandchild");
    assert!(validate_active_turns(&context).is_ok());
    assert!(validate_thread_parents(&context).is_ok());
}
#[test]
fn old_completion_cannot_remove_an_auto_started_goal_turn() {
    use codeterm_plugin::agent_runtime::*;
    let mut context = RuntimeContext {
        thread_id: Some("root".into()),
        ..Default::default()
    };
    for (turn, state) in [
        ("old", RuntimeTurnState::Working),
        ("new", RuntimeTurnState::Working),
        ("old", RuntimeTurnState::Idle),
    ] {
        observe_turn(
            &mut context,
            &AgentRuntimeEvent::TurnState {
                thread_id: "root".into(),
                turn_id: Some(turn.into()),
                parent_thread_id: None,
                state,
                started_at: None,
                completed_at: None,
            },
        );
    }
    assert_eq!(context.active_turns[0].turn_id, "new");
}
