use super::*;
#[test]
fn sign_in_required_waits_in_the_owned_epoch_and_preserves_thread_identity() {
    let mut state = RuntimeState::unloaded(Some("opaque-thread".into()));
    let epoch = state.begin();
    assert!(!state.degrade(epoch + 1, &RuntimeFailure::SignInRequired));
    assert!(state.degrade(epoch, &RuntimeFailure::SignInRequired));
    assert_eq!(state.snapshot().state, "loginPending");
    assert!(state.snapshot().reason.is_none());
    assert!(!state.load(epoch, "foreign-thread"));
    assert!(state.load(epoch, "opaque-thread"));
    assert_eq!(state.snapshot().state, "loaded");
    assert_eq!(state.epoch, epoch);
}
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
    assert!(cache.tail(0).is_empty());
    assert_eq!(cache.tail(1)[0].id, "b");
    assert_eq!(cache.tail(1)[0].seq, 1);
    assert_eq!(cache.tail(10).len(), 2);
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
        item_id: Some("user-item".into()),
        thread_id: "own".into(),
        send_id: "send".into(),
        turn_id: "turn".into(),
    };
    assert!(validated_evidence(&[claim.clone()], &[evidence.clone()]).is_empty());
    let mut row = ChatMsg {
        id: "user-item".into(),
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
            .contains(&("send".into(), Some("user-item".into())))
    );
    let mut wrong_evidence = evidence.clone();
    if let AgentRuntimeEvent::SendEvidence { item_id, .. } = &mut wrong_evidence {
        *item_id = Some("unobserved-item".into());
    }
    let accepted = validated_evidence(
        &[claim.clone()],
        &[event(row.clone()), evidence.clone(), wrong_evidence],
    );
    assert_eq!(
        accepted,
        [("send".into(), Some("user-item".into()))]
            .into_iter()
            .collect()
    );
    let mut wrong_id = row.clone();
    wrong_id.id = "other-user-item".into();
    assert!(validated_evidence(&[claim.clone()], &[event(wrong_id), evidence.clone()]).is_empty());
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
fn resume_idle_without_a_turn_cannot_settle_a_turn_started_after_the_request() {
    use codeterm_plugin::agent_runtime::*;
    let before = RuntimeContext::default();
    let mut current = before.clone();
    let idle = AgentRuntimeEvent::TurnState {
        thread_id: "own".into(),
        turn_id: None,
        parent_thread_id: None,
        state: RuntimeTurnState::Idle,
        detail: None,
        status_line: None,
        started_at: None,
        completed_at: None,
    };
    assert!(accepts_response_event(&before, &current, &idle));
    current.active_turn_id = Some("newer".into());
    assert!(!accepts_response_event(&before, &current, &idle));
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
            detail: None,
            status_line: None,
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
                detail: None,
                status_line: None,
            },
        );
    }
    assert_eq!(context.active_turns[0].turn_id, "new");
}

#[test]
fn failure_display_preserves_supplied_details_without_debug_wrapper() {
    let detail = "opaque supplied failure";
    let failures = [
        RuntimeFailure::SpawnFailed {
            reason: detail.into(),
        },
        RuntimeFailure::ProcessExited {
            code: Some(7),
            signal: None,
            stderr: detail.into(),
            requested: false,
        },
        RuntimeFailure::ProcessExited {
            code: None,
            signal: Some(9),
            stderr: detail.into(),
            requested: false,
        },
        RuntimeFailure::PipeFailure {
            stream: "stdout".into(),
            reason: detail.into(),
        },
        RuntimeFailure::InvalidFrame(detail.into()),
        RuntimeFailure::HookFailed(detail.into()),
        RuntimeFailure::Refused(detail.into()),
        RuntimeFailure::PrewriteRefused(detail.into()),
    ];
    for failure in failures {
        let display = failure.to_string();
        assert!(display.contains(detail));
        assert_ne!(display, format!("{failure:?}"));
    }
    for failure in [
        RuntimeFailure::PipeEof,
        RuntimeFailure::Deadline,
        RuntimeFailure::Superseded,
        RuntimeFailure::ProcessExited {
            code: None,
            signal: None,
            stderr: String::new(),
            requested: true,
        },
    ] {
        assert!(!failure.to_string().is_empty());
        assert_ne!(failure.to_string(), format!("{failure:?}"));
    }
}

#[test]
fn live_tool_elapsed_boundaries_follow_events_and_survive_updates() {
    let mut cache = ItemCache::default();
    let row = |status: &str| ChatMsg {
        id: "call".into(),
        tool_status: Some(status.into()),
        ..Default::default()
    };
    let start = cache.reconcile_live(row("running"), 1_000);
    assert_eq!(start.tool_started_at_ms, Some(1_000));
    let progress = cache.reconcile_live(row("running"), 601_000);
    assert_eq!(progress.tool_started_at_ms, Some(1_000));
    assert_eq!(progress.tool_completed_at_ms, None);
    let abandoned = cache.reconcile_live(row("abandoned"), 602_000);
    assert_eq!(abandoned.tool_started_at_ms, Some(1_000));
    assert_eq!(abandoned.tool_completed_at_ms, Some(602_000));
    assert_eq!(cache.snapshot().len(), 1);
}

fn observe_turn(
    context: &mut codeterm_plugin::agent_runtime::RuntimeContext,
    event: &codeterm_plugin::agent_runtime::AgentRuntimeEvent,
) {
    use crate::service::agent_runtime::events::{observe, translate};
    observe(context, &translate(event));
}

#[test]
fn rereadable_cache_evicts_rows_and_indexes_and_bounds_replacements() {
    let mut cache = ItemCache::default();
    cache.bound_rereadable();
    for index in 0..1000 {
        cache.reconcile_live(
            ChatMsg {
                id: index.to_string(),
                content: "x".repeat(32),
                ..Default::default()
            },
            1,
        );
    }
    assert_eq!(cache.items.len(), REREADABLE_CACHE_ROWS);
    assert_eq!(cache.positions.len(), REREADABLE_CACHE_ROWS);
    assert_eq!(cache.items.front().unwrap().id, "600");
    for index in 0..8 {
        cache.reconcile(ChatMsg {
            id: (999 - index).to_string(),
            content: "x".repeat(1024 * 1024),
            ..Default::default()
        });
        assert!(cache.cached_bytes <= REREADABLE_CACHE_BYTES);
    }
    cache.reconcile(ChatMsg {
        id: "oversized".into(),
        content: "x".repeat(REREADABLE_CACHE_BYTES + 1),
        ..Default::default()
    });
    assert!(cache.items.is_empty());
    assert!(cache.positions.is_empty());
    assert!(cache.history_origin.is_empty());
    assert!(cache.live_during_backfill.is_empty());
    assert_eq!(cache.cached_bytes, 0);
}

#[test]
fn rereadable_backfill_keeps_newest_rows_and_can_reread_evicted_ids() {
    let mut cache = ItemCache::default();
    cache.bound_rereadable();
    for page in (0..10).rev() {
        cache.reconcile_backfill(
            (page * 100..(page + 1) * 100)
                .map(|index| ChatMsg {
                    id: index.to_string(),
                    content: "history".into(),
                    ..Default::default()
                })
                .collect(),
        );
        assert!(cache.items.len() <= REREADABLE_CACHE_ROWS);
        assert!(cache.history_origin.len() <= REREADABLE_CACHE_ROWS);
    }
    assert_eq!(cache.items.front().unwrap().id, "600");
    cache.begin_backfill();
    assert!(cache.items.is_empty());
    cache.reconcile_backfill(vec![ChatMsg {
        id: "0".into(),
        ..Default::default()
    }]);
    assert_eq!(cache.items.front().unwrap().id, "0");
}

#[test]
fn identity_free_provider_evidence_requires_its_exact_live_send_claim() {
    use codeterm_plugin::agent_runtime::{AgentRuntimeEvent, ClaimedSend, TurnInput};
    let claim = ClaimedSend {
        send_id: "send".into(),
        thread_id: "own".into(),
        input: TurnInput {
            text: "payload".into(),
            attachments: Vec::new(),
        },
    };
    let evidence: AgentRuntimeEvent = serde_json::from_value(serde_json::json!({
        "kind":"sendEvidence", "threadId":"own", "sendId":"send", "turnId":"turn"
    }))
    .unwrap();
    assert_eq!(
        validated_evidence(&[claim.clone()], &[evidence.clone()]),
        [("send".into(), None)].into_iter().collect()
    );
    assert!(validated_evidence(&[], &[evidence.clone()]).is_empty());
    let foreign = ClaimedSend {
        thread_id: "foreign".into(),
        ..claim
    };
    assert!(validated_evidence(&[foreign], &[evidence]).is_empty());
}

#[test]
fn responsiveness_selection_and_metrics_preserve_exact_authority_without_bodies() {
    use codeterm_plugin::agent_runtime::{RuntimeContext, RuntimeFrame};
    let selection: responsiveness::Selection = serde_json::from_value(serde_json::json!({
        "trace":"opaque-trace", "tab":"tab", "session":"session", "nonce":"nonce",
        "epoch":7, "turn":"turn", "item":"item"
    })).unwrap();
    assert!(selection.valid());
    let context = RuntimeContext { tab_id:"tab".into(), thread_id:Some("session".into()),
        binding_nonce:"nonce".into(), epoch:7, ..RuntimeContext::default() };
    assert!(selection.matches(&context));
    for foreign in [RuntimeContext { epoch:8, ..context.clone() },
        RuntimeContext { binding_nonce:"other".into(), ..context.clone() },
        RuntimeContext { thread_id:Some("other".into()), ..context.clone() },
        RuntimeContext { tab_id:"other".into(), ..context.clone() }] {
        assert!(!selection.matches(&foreign));
    }
    let frame = RuntimeFrame::Notification { method:"opaque/method".into(), params:serde_json::json!({
        "threadId":"session", "turnId":"turn", "itemId":"item", "delta":"\u{03bb}",
        "secret":"opaque-body-must-not-escape"
    }) };
    let metrics = selection.frame_metrics(&frame).unwrap();
    assert_eq!(metrics["delta_bytes"],2);
    assert_eq!(metrics.as_object().unwrap().len(),4);
    assert!(!metrics.to_string().contains("opaque-body"));
    let foreign = RuntimeFrame::Notification { method:"opaque/method".into(), params:serde_json::json!({
        "threadId":"other", "turnId":"turn", "itemId":"item", "delta":"opaque"
    }) };
    assert!(selection.frame_metrics(&foreign).is_none());
    let missing = RuntimeFrame::Notification { method:"opaque/method".into(), params:serde_json::json!({
        "threadId":"session", "itemId":"item", "delta":"opaque"
    }) };
    assert!(selection.frame_metrics(&missing).is_none());
}

#[test]
fn responsiveness_state_byte_helper_keeps_the_existing_limit_policy() {
    let state = serde_json::json!({"opaque":"\u{03bb}"});
    assert_eq!(validated_state_bytes(&state).unwrap(),serde_json::to_vec(&state).unwrap().len());
    assert!(validate_state_size(&state).is_ok());
    let oversized = serde_json::Value::String("x".repeat(codeterm_plugin::agent_runtime::MAX_RUNTIME_STATE_BYTES));
    assert!(matches!(validated_state_bytes(&oversized),Err(RuntimeFailure::Refused(_))));
    assert!(matches!(validate_state_size(&oversized),Err(RuntimeFailure::Refused(_))));
}

#[test]
fn responsiveness_native_record_cap_and_disposal_are_observational() {
    let selection: responsiveness::Selection = serde_json::from_value(serde_json::json!({
        "trace":"opaque-trace", "tab":"tab", "session":"session", "nonce":"nonce",
        "epoch":7, "turn":"turn", "item":"item"
    })).unwrap();
    let trace = responsiveness::Trace::new(selection);
    for _ in 0..2050 { trace.mark("fixture",None,serde_json::json!({"count":1})); }
    assert_eq!(trace.counts(),(2050,2,false));
    trace.dispose();
    assert_eq!(trace.counts(),(2051,2,true));
    trace.mark("fixture",None,serde_json::json!({"count":2}));
    trace.dispose();
    assert_eq!(trace.counts(),(2051,2,true));
}

#[test]
fn responsiveness_first_binding_latches_without_following_a_replacement_epoch() {
    use codeterm_plugin::agent_runtime::RuntimeContext;
    let selection: responsiveness::Selection = serde_json::from_value(serde_json::json!({
        "trace":"opaque-trace", "tab":"tab", "session":"session", "item_type":"commandExecution"
    })).unwrap();
    assert!(selection.valid());
    let trace = responsiveness::Trace::new(selection);
    let context = RuntimeContext { tab_id:"tab".into(), thread_id:Some("session".into()),
        binding_nonce:"first".into(), epoch:4, ..RuntimeContext::default() };
    assert!(trace.bind(&context));
    assert!(trace.bind(&context));
    assert!(!trace.bind(&RuntimeContext { epoch:5, ..context.clone() }));
    assert!(!trace.bind(&RuntimeContext { binding_nonce:"second".into(), ..context.clone() }));
    assert!(!trace.bind(&RuntimeContext { thread_id:Some("foreign".into()), ..context }));
}

#[test]
fn responsiveness_first_live_item_does_not_follow_foreign_or_later_items() {
    use codeterm_plugin::agent_runtime::{RuntimeContext, RuntimeFrame};
    let selection: responsiveness::Selection = serde_json::from_value(serde_json::json!({
        "trace":"opaque-trace", "tab":"tab", "session":"session", "item_type":"commandExecution"
    })).unwrap();
    let trace = responsiveness::Trace::new(selection);
    assert!(trace.bind(&RuntimeContext { tab_id:"tab".into(), thread_id:Some("session".into()),
        binding_nonce:"nonce".into(), epoch:1, ..RuntimeContext::default() }));
    let started = |thread: &str, id: &str, kind: &str| RuntimeFrame::Notification {
        method:"item/started".into(), params:serde_json::json!({"threadId":thread,"turnId":"turn",
            "item":{"id":id,"type":kind,"text":"opaque-body"}})
    };
    trace.frame(&started("session","user","userMessage"));
    trace.frame(&started("foreign","item","commandExecution"));
    assert_eq!(trace.counts(),(0,0,false));
    trace.frame(&started("session","item","commandExecution"));
    assert_eq!(trace.counts(),(1,0,false));
    trace.frame(&started("session","different","commandExecution"));
    assert_eq!(trace.counts(),(1,0,false));
    trace.frame(&RuntimeFrame::Notification { method:"item/commandExecution/outputDelta".into(),
        params:serde_json::json!({"threadId":"session","turnId":"turn","itemId":"item","delta":"opaque"}) });
    assert_eq!(trace.counts(),(2,0,false));
}
