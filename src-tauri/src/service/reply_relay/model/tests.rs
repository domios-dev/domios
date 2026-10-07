use super::*;

fn pending() -> PendingReplyRelay {
    PendingReplyRelay {
        sender_tab: "sender".into(),
        delivery_id: "delivery-1".into(),
        provider_id: "provider".into(),
        session_id: "own-session".into(),
        binding_nonce: "launch".into(),
        after_offset: 100,
        user_text: "opaque payload".into(),
        turn_normalization: Default::default(),
        hops: 0,
        task: None,
    }
}

fn source() -> RelaySource<'static> {
    RelaySource {
        provider_id: "provider",
        session_id: "own-session",
        binding_nonce: "launch",
    }
}

fn completed() -> ReplyRelayTurn {
    ReplyRelayTurn {
        user_turn_id: "user-100".into(),
        user_record_start: 100,
        user_text: "opaque payload".into(),
        assistant_turn_id: "assistant-120".into(),
        answer: "final answer".into(),
        complete: true,
    }
}

#[test]
fn relay_eligibility_requires_local_agent_route_reply_hint_and_capability() {
    assert!(reply_relay_eligible(true, true, true, true, false));
    for args in [
        (false, true, true, true, false),
        (true, false, true, true, false),
        (true, true, false, true, false),
        (true, true, true, false, false),
        (true, true, true, true, true),
    ] {
        assert!(!reply_relay_eligible(
            args.0, args.1, args.2, args.3, args.4
        ));
    }
}

#[test]
fn only_exact_own_session_turn_after_captured_boundary_can_relay() {
    let pending = pending();
    let turn = completed();
    assert_eq!(
        decide_reply_relay(&pending, source(), Some(&turn), true, 8, false),
        ReplyRelayDecision::Send {
            answer: "final answer".into(),
            hops: 1,
            user_turn_id: "user-100".into()
        }
    );
    for foreign in [
        RelaySource {
            provider_id: "other",
            ..source()
        },
        RelaySource {
            session_id: "foreign",
            ..source()
        },
        RelaySource {
            binding_nonce: "old-launch",
            ..source()
        },
    ] {
        assert_eq!(
            decide_reply_relay(&pending, foreign, Some(&turn), true, 8, false),
            ReplyRelayDecision::Wait
        );
    }
    let stale = ReplyRelayTurn {
        user_record_start: 99,
        ..turn.clone()
    };
    let different = ReplyRelayTurn {
        user_text: "different payload".into(),
        ..turn
    };
    for rejected in [stale, different] {
        assert_eq!(
            decide_reply_relay(&pending, source(), Some(&rejected), true, 8, false),
            ReplyRelayDecision::Wait
        );
    }
}

#[test]
fn absent_incomplete_or_unidentified_answer_never_relays() {
    let pending = pending();
    assert_eq!(
        decide_reply_relay(&pending, source(), None, true, 8, false),
        ReplyRelayDecision::Wait
    );
    for turn in [
        ReplyRelayTurn {
            complete: false,
            ..completed()
        },
        ReplyRelayTurn {
            user_turn_id: String::new(),
            ..completed()
        },
        ReplyRelayTurn {
            assistant_turn_id: String::new(),
            ..completed()
        },
        ReplyRelayTurn {
            answer: "  ".into(),
            ..completed()
        },
    ] {
        assert_eq!(
            decide_reply_relay(&pending, source(), Some(&turn), true, 8, false),
            ReplyRelayDecision::Wait
        );
    }
}

#[test]
fn hop_bound_and_closed_sender_drop_pending_relay() {
    let mut pending = pending();
    pending.hops = 7;
    assert!(matches!(
        decide_reply_relay(&pending, source(), Some(&completed()), true, 8, false),
        ReplyRelayDecision::Send { hops: 8, .. }
    ));
    pending.hops = 8;
    assert_eq!(
        decide_reply_relay(&pending, source(), Some(&completed()), true, 8, false),
        ReplyRelayDecision::DropHopLimit
    );
    pending.hops = u8::MAX;
    assert_eq!(
        decide_reply_relay(&pending, source(), Some(&completed()), true, u8::MAX, false),
        ReplyRelayDecision::DropHopLimit
    );
    assert_eq!(
        decide_reply_relay(&pending, source(), None, false, 8, false),
        ReplyRelayDecision::DropClosedSender
    );
}

#[test]
fn one_completed_turn_consumes_only_one_pending_delivery_and_never_replays() {
    let mut model = ReplyRelayModel::default();
    assert!(model.record(pending()));
    assert!(!model.record(pending()));
    let mut second = pending();
    second.delivery_id = "delivery-2".into();
    assert!(model.record(second));
    let actions = model.observe(source(), Some(&completed()), |_| true, 8);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].0.delivery_id, "delivery-1");
    assert!(!model.record(pending()));
    assert!(model
        .observe(source(), Some(&completed()), |_| true, 8)
        .is_empty());
    let next = ReplyRelayTurn {
        user_turn_id: "user-200".into(),
        user_record_start: 200,
        ..completed()
    };
    let actions = model.observe(source(), Some(&next), |_| true, 8);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].0.delivery_id, "delivery-2");
    assert!(model.observe(source(), Some(&next), |_| true, 8).is_empty());
}

#[test]
fn closed_sender_removes_pending_without_waiting_for_answer() {
    let mut model = ReplyRelayModel::default();
    model.record(pending());
    let actions = model.observe(source(), None, |_| false, 8);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].1, ReplyRelayDecision::DropClosedSender);
    assert!(model
        .observe(source(), Some(&completed()), |_| true, 8)
        .is_empty());
}

#[test]
fn exact_turn_matching_reuses_provider_declared_delivery_normalization() {
    let mut pending = pending();
    pending.user_text = "opaque  payload".into();
    let turn = completed();
    assert_eq!(
        decide_reply_relay(&pending, source(), Some(&turn), true, 8, false),
        ReplyRelayDecision::Wait
    );
    pending.turn_normalization.collapse_horizontal_whitespace = true;
    assert!(matches!(
        decide_reply_relay(&pending, source(), Some(&turn), true, 8, false),
        ReplyRelayDecision::Send { .. }
    ));
}

#[test]
fn relay_model_refuses_capacity_without_evicting_consumed_delivery_ids() {
    let mut model = ReplyRelayModel::default();
    for index in 0..MAX_TRACKED_RELAY_DELIVERIES {
        let mut entry = pending();
        entry.delivery_id = format!("delivery-{index}");
        assert!(model.record(entry));
    }
    let mut extra = pending();
    extra.delivery_id = "overflow".into();
    assert!(!model.record(extra));
    let actions = model.observe(source(), None, |_| false, DEFAULT_REPLY_RELAY_MAX_HOPS);
    assert_eq!(actions.len(), MAX_TRACKED_RELAY_DELIVERIES);
    assert!(!model.record(pending()));
}

#[test]
fn missing_launch_identity_cannot_authorize_relay_even_when_both_sides_are_empty() {
    let mut pending = pending();
    pending.binding_nonce.clear();
    let source = RelaySource {
        binding_nonce: "",
        ..source()
    };
    assert_eq!(
        decide_reply_relay(&pending, source, Some(&completed()), true, 8, false),
        ReplyRelayDecision::Wait
    );
}

fn ended(outcome: crate::agent::provider::ReplyRelayOutcome) -> ReplyRelayEvidence {
    ReplyRelayEvidence::Ended(crate::agent::provider::ReplyRelayEndedTurn {
        user_turn_id: "user-100".into(),
        user_record_start: 100,
        user_text: "opaque payload".into(),
        complete: false,
        outcome,
    })
}

#[test]
fn non_success_turns_notify_once_without_answer_text() {
    use crate::agent::provider::ReplyRelayOutcome;
    for outcome in [
        ReplyRelayOutcome::Error,
        ReplyRelayOutcome::Cancelled,
        ReplyRelayOutcome::ReflectionLimit,
    ] {
        let evidence = ended(outcome.clone());
        let mut model = ReplyRelayModel::default();
        assert!(model.record(pending()));
        let actions = model.observe_evidence(source(), Some(&evidence), |_| true, 8);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].1,
            ReplyRelayDecision::Notice {
                text: format!("turn ended: {}", outcome.as_str()),
                user_turn_id: Some("user-100".into()),
            }
        );
        assert!(model
            .observe_evidence(source(), Some(&evidence), |_| true, 8)
            .is_empty());
    }
}

#[test]
fn non_success_requires_exact_user_and_launch_evidence() {
    use crate::agent::provider::ReplyRelayOutcome;
    let mut evidence = ended(ReplyRelayOutcome::Error);
    if let ReplyRelayEvidence::Ended(turn) = &mut evidence {
        turn.user_text = "foreign".into();
    }
    assert_eq!(
        decide_reply_relay_evidence(&pending(), source(), Some(&evidence), true, 8, false),
        ReplyRelayDecision::Wait
    );
    let evidence = ended(ReplyRelayOutcome::Error);
    let foreign = RelaySource {
        binding_nonce: "other",
        ..source()
    };
    assert_eq!(
        decide_reply_relay_evidence(&pending(), foreign, Some(&evidence), true, 8, false),
        ReplyRelayDecision::Wait
    );
}

#[test]
fn unavailable_notifies_pending_senders_and_disables_this_binding() {
    let evidence = ReplyRelayEvidence::Unavailable(crate::agent::provider::ReplyRelayUnavailable {
        unavailable: true,
        reason: "adapter not active".into(),
    });
    let mut model = ReplyRelayModel::default();
    assert!(model.record(pending()));
    let mut second = pending();
    second.delivery_id = "delivery-2".into();
    second.sender_tab = "other".into();
    assert!(model.record(second));
    let actions = model.observe_evidence(source(), Some(&evidence), |_| true, 8);
    assert_eq!(actions.len(), 2);
    assert!(actions.iter().all(|(_, decision)| matches!(
        decision,
        ReplyRelayDecision::Notice {
            user_turn_id: None,
            ..
        }
    )));
    assert_eq!(model.unavailable_reason(), Some("adapter not active"));
    let mut later = pending();
    later.delivery_id = "later".into();
    assert!(!model.record(later));
    assert!(model
        .observe_evidence(source(), Some(&evidence), |_| true, 8)
        .is_empty());
    assert!(ReplyRelayModel::default().record(pending()));
}

#[test]
fn malformed_activation_and_completion_do_not_consume_pending_relay() {
    let mut turn = completed();
    turn.complete = false;
    for evidence in [
        ReplyRelayEvidence::Answered(turn),
        ReplyRelayEvidence::Unavailable(crate::agent::provider::ReplyRelayUnavailable {
            unavailable: false,
            reason: "invalid".into(),
        }),
    ] {
        let mut model = ReplyRelayModel::default();
        assert!(model.record(pending()));
        assert!(model
            .observe_evidence(source(), Some(&evidence), |_| true, 8)
            .is_empty());
        assert_eq!(model.unavailable_reason(), None);
        assert_eq!(
            model
                .observe(source(), Some(&completed()), |_| true, 8)
                .len(),
            1
        );
    }
}

#[test]
fn routed_messages_and_spawn_briefs_reply_to_the_agent_sender() {
    let route = ReplyRelayRoute {
        sender: "spawner",
        target: "worker",
        tool_less: true,
        supported: true,
        sender_is_agent: true,
        has_reply_hint: true,
        remote: false,
        notice: false,
        spawn_brief: false,
    };
    assert_eq!(reply_relay_target(route), Some("spawner"));
    assert_eq!(
        reply_relay_target(ReplyRelayRoute {
            spawn_brief: true,
            has_reply_hint: false,
            ..route
        }),
        Some("spawner")
    );
    for excluded in [
        ReplyRelayRoute {
            tool_less: false,
            ..route
        },
        ReplyRelayRoute {
            supported: false,
            ..route
        },
        ReplyRelayRoute {
            sender_is_agent: false,
            ..route
        },
        ReplyRelayRoute {
            remote: true,
            ..route
        },
        ReplyRelayRoute {
            notice: true,
            ..route
        },
        ReplyRelayRoute {
            has_reply_hint: false,
            ..route
        },
        ReplyRelayRoute {
            sender: "worker",
            ..route
        },
    ] {
        assert_eq!(reply_relay_target(excluded), None);
    }
}

#[test]
fn cancelled_before_write_delivery_cannot_relay() {
    let mut model = ReplyRelayModel::default();
    assert!(model.record(pending()));
    model.cancel("delivery-1");
    assert!(model
        .observe(source(), Some(&completed()), |_| true, 8)
        .is_empty());
}

#[test]
fn consumed_turn_watermark_keeps_long_sessions_bounded_without_relay_replay() {
    let mut model = ReplyRelayModel::default();
    for index in 0..(MAX_TRACKED_RELAY_DELIVERIES * 2) {
        let mut pending = pending();
        pending.delivery_id = format!("delivery-{index}");
        pending.after_offset = index as u64;
        let mut turn = completed();
        turn.user_record_start = index as u64;
        turn.user_turn_id = format!("turn-{index}");
        assert!(model.record(pending));
        assert_eq!(model.observe(source(), Some(&turn), |_| true, 8).len(), 1);
    }
    assert_eq!(model.delivery_ids.len(), MAX_TRACKED_RELAY_DELIVERIES);
    let mut later = pending();
    later.delivery_id = "new-delivery".into();
    assert!(model.record(later));
    assert!(model
        .observe(source(), Some(&completed()), |_| true, 8)
        .is_empty());
}

#[test]
fn unrelated_own_completed_turn_advances_cursor_without_relaying_or_confirming() {
    let pending = pending();
    let mut model = ReplyRelayModel::default();
    assert!(model.record(pending.clone()));
    let mut blank = completed();
    blank.user_text = "opaque unrelated turn".into();
    let evidence = ReplyRelayEvidence::Answered(blank);
    let foreign = RelaySource {
        session_id: "foreign",
        ..source()
    };
    assert!(!model.advance_past_unrelated_turn(&pending, foreign, Some(&evidence)));
    assert!(model.advance_past_unrelated_turn(&pending, source(), Some(&evidence)));
    assert_eq!(model.read_offset(&pending), 101);
    assert!(!model.advance_past_unrelated_turn(&pending, source(), Some(&evidence)));
    assert_eq!(model.pending().len(), 1);
    let mut answer = completed();
    answer.user_record_start = 200;
    answer.user_turn_id = "user-200".into();
    let evidence = ReplyRelayEvidence::Answered(answer);
    assert!(!model.advance_past_unrelated_turn(&pending, source(), Some(&evidence)));
    assert!(matches!(
        model.observe_delivery_evidence(
            &pending.delivery_id,
            source(),
            Some(&evidence),
            |_| true,
            8
        )[0]
        .1,
        ReplyRelayDecision::Send { .. }
    ));
    assert!(model.pending().is_empty());
}

#[test]
fn declared_line_break_normalization_relays_the_exact_stored_turn_once() {
    let mut pending = pending();
    pending.user_text =
        "<domios from=\"tab\" tab=\"sender\">opaque\r\npayload\n [hint]</domios>".into();
    let mut turn = completed();
    turn.user_text = pending.user_text.replace(['\r', '\n'], "");
    let mut model = ReplyRelayModel::default();
    assert!(model.record(pending.clone()));
    assert!(model.observe(source(), Some(&turn), |_| true, 8).is_empty());
    pending.turn_normalization.drop_line_breaks = true;
    let mut model = ReplyRelayModel::default();
    assert!(model.record(pending));
    let actions = model.observe(source(), Some(&turn), |_| true, 8);
    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0].1, ReplyRelayDecision::Send { .. }));
    assert!(model.observe(source(), Some(&turn), |_| true, 8).is_empty());
}
