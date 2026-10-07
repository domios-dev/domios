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
