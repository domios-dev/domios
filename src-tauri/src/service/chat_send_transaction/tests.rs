use super::*;
use crate::service::delivery_error::{DeliveryError, SendOrigin};
use crate::service::inject_verify::InjectionError;

fn submit_unconfirmed(detail: impl Into<String>) -> DeliveryError {
    DeliveryError::SubmitUnconfirmed {
        tab: "tab-1".into(),
        detail: detail.into(),
    }
}

fn tab_busy(detail: impl Into<String>) -> DeliveryError {
    DeliveryError::TabBusyRefused {
        tab: "tab-1".into(),
        detail: detail.into(),
    }
}

fn pty_unavailable(detail: impl Into<String>) -> DeliveryError {
    DeliveryError::PtyUnavailable {
        tab: "tab-1".into(),
        detail: detail.into(),
    }
}

fn reconciled_state(ledger: &ChatSendLedger, send_id: &str) -> Option<ChatSendState> {
    ledger
        .reconcile("tab-1")
        .into_iter()
        .find(|status| status.send_id == send_id)
        .map(|status| status.state)
}

#[test]
fn user_send_booting_agent_degrades_to_pending_not_failed() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hello", None).unwrap();
    let mut writes = 0;
    // Exact neutralization start_chat_send applies at the user-origin boundary.
    let status = run_chat_send(&ledger, pending, "hello", |_text| {
        writes += 1;
        Err(tab_busy("Agent in Tab tab-1 is still booting"))
            .or_else(|e: DeliveryError| e.into_surfaced(SendOrigin::User))
    });
    assert_eq!(writes, 1, "submit invoked exactly once — no retry loop");
    assert_eq!(status.state, ChatSendState::Unknown);
    assert!(status.error.is_none());
    assert_eq!(ledger.reconcile("tab-1")[0].state, ChatSendState::Unknown);
}

#[test]
fn user_send_unlinked_pty_degrades_to_pending_not_failed() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hello", None).unwrap();
    let status = run_chat_send(&ledger, pending, "hello", |_text| {
        Err(pty_unavailable("PTY did not link within 15s"))
            .or_else(|e: DeliveryError| e.into_surfaced(SendOrigin::User))
    });
    assert_eq!(status.state, ChatSendState::Unknown);
    assert!(status.error.is_none());
}

#[test]
fn owner_prewrite_revalidation_keeps_pending_reason_visible() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hello", None).unwrap();
    let authority = crate::service::pending_delivery_payloads::user_sends::UserSendAuthority {
        tab_id: "tab-1".into(),
        pty_id: Some("pty-1".into()),
        binding_nonce: "nonce-1".into(),
        pty_session_generation: Some(1),
        provider_id: "provider".into(),
        session_id: None,
        observed_session_id: Some("session-1".into()),
        prelaunch: false,
    };
    ledger
        .state
        .lock()
        .owned
        .insert(pending.send_id.clone(), (authority, true));
    ledger.set_user_pending_reason(
        &pending.send_id,
        Some("readiness_changed_before_payload_write"),
    );

    let status = ledger
        .finish(
            &pending.send_id,
            Err("zero_write: readiness changed".into()),
        )
        .unwrap();

    assert_eq!(status.state, ChatSendState::Pending);
    assert_eq!(status.error, None);
    assert_eq!(
        status.pending_reason.as_deref(),
        Some("readiness_changed_before_payload_write")
    );
}

#[test]
fn withheld_enter_keeps_the_waiting_on_prompt_reason_on_the_unconfirmed_send() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hello", None).unwrap();
    let authority = crate::service::pending_delivery_payloads::user_sends::UserSendAuthority {
        tab_id: "tab-1".into(),
        pty_id: Some("pty-1".into()),
        binding_nonce: "nonce-1".into(),
        pty_session_generation: Some(1),
        provider_id: "provider".into(),
        session_id: None,
        observed_session_id: Some("session-1".into()),
        prelaunch: false,
    };
    {
        let mut state = ledger.state.lock();
        state
            .owned
            .insert(pending.send_id.clone(), (authority, true));
        state.by_id.get_mut(&pending.send_id).unwrap().state = ChatSendState::Written;
    }
    ledger.note_waiting_on_prompt(&pending.send_id);

    let status = ledger
        .finish(
            &pending.send_id,
            Err("submit_unconfirmed: waiting_on_prompt".into()),
        )
        .unwrap();

    assert_eq!(status.state, ChatSendState::Unknown);
    assert_eq!(status.error, None);
    assert_eq!(
        status.pending_reason.as_deref(),
        Some(crate::service::pty_io::submit_transaction::WAITING_ON_PROMPT)
    );
}

#[test]
fn user_submit_unconfirmed_neither_fails_nor_rewrites() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hi", None).unwrap();
    let mut writes = 0;
    let status = run_chat_send(&ledger, pending, "hi", |_text| {
        writes += 1;
        Err(submit_unconfirmed("submit key not acknowledged"))
            .or_else(|e: DeliveryError| e.into_surfaced(SendOrigin::User))
    });
    assert_eq!(
        writes, 1,
        "write count stays 1 — submit recovery never re-writes"
    );
    assert_eq!(status.state, ChatSendState::Unknown);
    assert!(status.error.is_none());
}

#[test]
fn user_send_turn_evidence_still_confirms_a_degraded_send() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "deploy", None).unwrap();
    run_chat_send(&ledger, pending.clone(), "deploy", |_text| {
        Err(tab_busy("still booting")).or_else(|e: DeliveryError| e.into_surfaced(SendOrigin::User))
    });
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "deploy")
        .expect("turn evidence confirms the pending user send once the PTY links");
    assert_eq!(confirmed.send_id, pending.send_id);
    assert_eq!(confirmed.state, ChatSendState::Sent);
}

#[test]
fn one_turn_confirms_three_ordered_unconfirmed_sends_without_separators() {
    let ledger = ChatSendLedger::default();
    let bodies = ["alpha-opaque", "beta-opaque", "gamma-opaque"];
    let sends = bodies
        .iter()
        .map(|body| {
            let pending = ledger.try_start("tab-1", body, None).unwrap();
            run_chat_send(&ledger, pending, body, |_| {
                Err(submit_unconfirmed("submit key was not acknowledged"))
            })
        })
        .collect::<Vec<_>>();
    let merged = bodies.concat();

    let delivered =
        ledger.confirm_delivered_user_turns_scoped("tab-1", "session-1", Some("nonce-1"), &merged);

    assert_eq!(
        delivered
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
        sends
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
    );
    assert!(delivered
        .iter()
        .all(|status| status.state == ChatSendState::Sent));
    assert!(ledger
        .confirm_delivered_user_turns_scoped("tab-1", "session-1", Some("nonce-1"), &merged,)
        .is_empty());
}

#[test]
fn merged_turn_matching_allows_whitespace_between_ordered_bodies() {
    let normalization = crate::agent::provider::TurnEvidenceNormalization::default();
    let candidates = [
        ("send-1".into(), "alpha".into(), normalization),
        ("send-2".into(), "beta".into(), normalization),
        ("send-3".into(), "gamma".into(), normalization),
    ];

    assert_eq!(
        matching_turn_send_ids("alpha\n\nbeta gamma", &candidates),
        vec![
            String::from("send-1"),
            String::from("send-2"),
            String::from("send-3"),
        ],
    );
}

#[test]
fn programmatic_pre_write_refusal_still_surfaces_as_failed() {
    // Same DeliveryError WITHOUT the user neutralization (the programmatic
    // path returns it typed) stays Failed — the split is real.
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hi", None).unwrap();
    let status = run_chat_send(&ledger, pending, "hi", |_| Err(tab_busy("still booting")));
    assert_eq!(status.state, ChatSendState::Failed);
}

#[test]
fn serialized_states_report_write_and_exact_delivery_progress() {
    assert_eq!(
        serde_json::to_string(&ChatSendState::Pending).unwrap(),
        "\"held\""
    );
    assert_eq!(
        serde_json::to_string(&ChatSendState::Written).unwrap(),
        "\"written\""
    );
    assert_eq!(
        serde_json::to_string(&ChatSendState::Unknown).unwrap(),
        "\"submit_unconfirmed\""
    );
    assert_eq!(
        serde_json::to_string(&ChatSendState::Sent).unwrap(),
        "\"delivered\""
    );
    assert_eq!(
        serde_json::to_string(&ChatSendState::Failed).unwrap(),
        "\"failed\""
    );
}

#[test]
fn queued_user_payload_is_not_released_as_a_zero_write() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "payload", None).unwrap();
    ledger.state.lock().owned.insert(
        pending.send_id.clone(),
        (
            crate::service::pending_delivery_payloads::user_sends::UserSendAuthority {
                tab_id: "tab-1".into(),
                pty_id: Some("pty-1".into()),
                binding_nonce: "nonce-1".into(),
                pty_session_generation: Some(1),
                provider_id: "provider".into(),
                session_id: None,
                observed_session_id: None,
                prelaunch: false,
            },
            true,
        ),
    );
    ledger.mark_user_written(&pending.send_id).unwrap();

    assert!(ledger.user_payload_may_have_been_written(&pending.send_id));
    assert!(!ledger.user_confirmed(&pending.send_id));
}

#[test]
fn unconfirmed_submit_retains_one_payload_and_correlated_result() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "payload", None).unwrap();
    let mut writes = Vec::new();
    let result = run_chat_send(&ledger, pending.clone(), "one payload", |text| {
        writes.push(text.to_string());
        Err(submit_unconfirmed("submit key was not acknowledged"))
    });

    assert_eq!(writes, ["one payload"]);
    assert_eq!(result.send_id, pending.send_id);
    assert_eq!(result.state, ChatSendState::Unknown);
    assert_eq!(result.error, None);
    assert_eq!(ledger.reconcile("tab-1"), vec![result]);
}

#[test]
fn unknown_transaction_survives_ui_detach_and_activation() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "payload", None).unwrap();

    assert_eq!(ledger.reconcile("tab-1"), vec![pending.clone()]);

    let result = run_chat_send(&ledger, pending, "hello", |_| Ok(()));

    assert_eq!(result.state, ChatSendState::Unknown);
    assert_eq!(ledger.reconcile("tab-1"), vec![result]);
}

#[test]
fn a_missed_failure_survives_a_newer_pending_send() {
    let ledger = ChatSendLedger::default();
    let first = ledger.try_start("tab-1", "payload", None).unwrap();
    let second = ledger.try_start("tab-1", "payload", None).unwrap();
    let failed = run_chat_send(&ledger, first, "first", |_| {
        Err(submit_unconfirmed("Enter swallowed"))
    });

    assert_eq!(
        reconciled_state(&ledger, &failed.send_id),
        Some(ChatSendState::Unknown)
    );
    assert_eq!(
        reconciled_state(&ledger, &second.send_id),
        Some(ChatSendState::Pending)
    );
}

#[test]
fn in_flight_cap_rejects_before_growth_and_releases_on_completion() {
    let ledger = ChatSendLedger::default();
    let mut pending = Vec::new();
    for index in 0..MAX_IN_FLIGHT {
        pending.push(
            ledger
                .try_start(&format!("tab-{index}"), "payload", None)
                .unwrap(),
        );
    }

    assert!(ledger.try_start("overflow", "payload", None).is_err());
    assert_eq!(ledger.state.lock().by_id.len(), MAX_IN_FLIGHT);

    ledger.finish(&pending[0].send_id, Ok(())).unwrap();
    assert!(ledger.try_start("released", "payload", None).is_ok());
    ledger
        .confirm_delivered_user_turn("tab-0", "session-0", "payload")
        .unwrap();
    assert_eq!(ledger.state.lock().by_id.len(), MAX_IN_FLIGHT + 1);
}

#[test]
fn reconcile_retains_every_unknown_send_missed_while_chat_is_detached() {
    let ledger = ChatSendLedger::default();
    let first = ledger.try_start("tab-1", "payload", None).unwrap();
    let second = ledger.try_start("tab-1", "payload", None).unwrap();
    run_chat_send(&ledger, first, "first", |_| {
        Err(submit_unconfirmed("first failed"))
    });
    run_chat_send(&ledger, second, "second", |_| {
        Err(submit_unconfirmed("second failed"))
    });

    let statuses = ledger.reconcile("tab-1");
    assert_eq!(statuses.len(), 2);
    assert!(statuses
        .iter()
        .all(|status| status.state == ChatSendState::Unknown && status.error.is_none()));
}

#[test]
fn reconcile_keeps_delivered_sends_alongside_unconfirmed_history() {
    let ledger = ChatSendLedger::default();
    let first = ledger.try_start("tab-1", "alpha", None).unwrap();
    let second = ledger.try_start("tab-1", "beta", None).unwrap();
    let third = ledger.try_start("tab-1", "gamma", None).unwrap();
    for (pending, body) in [(&first, "alpha"), (&second, "beta"), (&third, "gamma")] {
        run_chat_send(&ledger, pending.clone(), body, |_| Ok(()));
    }

    let delivered = ledger.confirm_delivered_user_turns_scoped(
        "tab-1",
        "session-1",
        Some("nonce-1"),
        "alpha\n\nbeta",
    );
    assert_eq!(delivered.len(), 2);
    assert!(delivered
        .iter()
        .all(|status| status.state == ChatSendState::Sent));

    for (send, state) in [
        (&first, ChatSendState::Sent),
        (&second, ChatSendState::Sent),
        (&third, ChatSendState::Unknown),
    ] {
        assert_eq!(reconciled_state(&ledger, &send.send_id), Some(state));
    }
}

#[test]
fn reconcile_keeps_latest_pending_send_with_delivered_history() {
    let ledger = ChatSendLedger::default();
    let delivered = ledger.try_start("tab-1", "delivered", None).unwrap();
    run_chat_send(&ledger, delivered.clone(), "delivered", |_| Ok(()));
    ledger.confirm_delivered_user_turn("tab-1", "session-1", "delivered");
    let pending = ledger.try_start("tab-1", "pending", None).unwrap();

    assert_eq!(
        reconciled_state(&ledger, &delivered.send_id),
        Some(ChatSendState::Sent)
    );
    assert_eq!(
        reconciled_state(&ledger, &pending.send_id),
        Some(ChatSendState::Pending)
    );
}

#[test]
fn reconciliation_exposes_the_backend_retention_window() {
    let ledger = ChatSendLedger::default();
    for index in 0..=MAX_RESULTS {
        let pending = ledger.try_start("tab-1", "payload", None).unwrap();
        run_chat_send(&ledger, pending, "message", |_| {
            Err(submit_unconfirmed(format!("failure-{index}")))
        });
    }

    let retained = ledger.reconcile("tab-1");
    assert_eq!(retained.len(), MAX_RESULTS);
    assert_eq!(retained[0].sequence, 2);
    assert!(retained.iter().all(|status| status.retained_from == 2));
}

#[test]
fn echo_mismatch_reaches_ledger_as_submit_unconfirmed() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "payload", None).unwrap();
    let result = run_chat_send(&ledger, pending, "wrapped payload", |_| {
        Err(DeliveryError::from_injection(
            "tab-1",
            InjectionError::EchoMismatch {
                expected_substr: "wrapped payload".into(),
                screen_tail: "bordered frame".into(),
            },
        ))
    });

    assert_eq!(result.state, ChatSendState::Unknown);
    assert_eq!(result.error, None);
}

#[test]
fn exact_turn_evidence_clears_a_false_failure() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "deploy the fix", None).unwrap();
    run_chat_send(&ledger, pending.clone(), "deploy the fix", |_| {
        Err(submit_unconfirmed("acknowledgement heuristics timed out"))
    });

    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "deploy the fix")
        .expect("exact same-tab turn evidence confirms the failed send");
    assert_eq!(confirmed.send_id, pending.send_id);
    assert_eq!(confirmed.state, ChatSendState::Sent);
    assert_eq!(confirmed.error, None);
    assert_eq!(ledger.reconcile("tab-1"), vec![confirmed]);
}

#[test]
fn typed_follow_up_is_part_of_exact_chat_turn_evidence() {
    let ledger = ChatSendLedger::default();
    let pending = ledger
        .try_start_scoped_with_normalization(
            "tab-1",
            "opaque-body suffix-token",
            None,
            None,
            Default::default(),
        )
        .unwrap();
    ledger.finish(&pending.send_id, Ok(())).unwrap();

    assert!(ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "opaque-body")
        .is_none());
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "opaque-body suffix-token")
        .expect("the exact typed follow-up is included in the user turn");

    assert_eq!(confirmed.send_id, pending.send_id);
    assert_eq!(confirmed.state, ChatSendState::Sent);
}

#[test]
fn declared_provider_normalization_confirms_only_the_complete_chat_payload() {
    let ledger = ChatSendLedger::default();
    let pending = ledger
        .try_start_scoped_with_normalization(
            "tab-1",
            "name\tvalue\n\talpha\tbeta",
            None,
            None,
            crate::agent::provider::TurnEvidenceNormalization {
                collapse_horizontal_whitespace: true,
                ..Default::default()
            },
        )
        .unwrap();
    ledger.finish(&pending.send_id, Ok(())).unwrap();
    assert!(ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "name value\n alpha")
        .is_none());
    let confirmed = ledger
        .confirm_delivered_user_turn(
            "tab-1",
            "session-1",
            "prefix\nname value\r\n alpha beta\nsuffix",
        )
        .unwrap();
    assert_eq!(confirmed.send_id, pending.send_id);
}

#[test]
fn late_lazy_session_first_turn_upgrades_written_once() {
    let ledger = ChatSendLedger::default();
    let pending = ledger
        .try_start_scoped_with_normalization(
            "tab-1",
            "Codex first turn marker",
            None,
            Some(SendLiveBinding {
                session_id: None,
                binding_nonce: Some("launch-1".into()),
            }),
            Default::default(),
        )
        .unwrap();
    assert!(ledger.set_deferred_first_turn_evidence(
        &pending.send_id,
        DeferredFirstTurnEvidence {
            provider_id: "capability-provider".into(),
            cwd: "/repo".into(),
            binding_nonce: "launch-1".into(),
        },
    ));
    ledger
        .state
        .lock()
        .by_id
        .get_mut(&pending.send_id)
        .unwrap()
        .state = ChatSendState::Written;

    let candidate = ledger
        .deferred_first_turn_candidates("tab-1")
        .into_iter()
        .next()
        .unwrap();
    let confirmed = ledger
        .confirm_deferred_first_turn(&candidate, "session-1")
        .expect("the current first user turn confirms the lazy first send");

    assert_eq!(confirmed.send_id, pending.send_id);
    assert_eq!(confirmed.state, ChatSendState::Sent);
    assert!(ledger.deferred_first_turn_candidates("tab-1").is_empty());
    assert!(ledger
        .confirm_deferred_first_turn(&candidate, "session-1")
        .is_none());
}

#[test]
fn evidence_during_flight_wins_over_late_transport_failure() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "restart dev", None).unwrap();
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "  restart dev  ")
        .expect("turn evidence may arrive before classification finishes");
    assert_eq!(confirmed.state, ChatSendState::Sent);

    let result = run_chat_send(&ledger, pending, "restart dev", |_| {
        Err(DeliveryError::from_injection(
            "tab-1",
            InjectionError::PtyClosed {
                pty_id: "pty-1".into(),
            },
        ))
    });
    assert_eq!(result.state, ChatSendState::Sent);
    assert_eq!(result.error, None);
    assert_eq!(ledger.reconcile("tab-1"), vec![result]);
}

#[test]
fn unrelated_stale_or_foreign_turns_never_confirm() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "exact payload", None).unwrap();
    run_chat_send(&ledger, pending, "exact payload", |_| {
        Err(submit_unconfirmed("no evidence"))
    });

    assert!(ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "different text")
        .is_none());
    assert!(ledger
        .confirm_delivered_user_turn("tab-2", "session-1", "exact payload")
        .is_none());
    assert!(ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "   ")
        .is_none());
    assert_eq!(ledger.reconcile("tab-1")[0].state, ChatSendState::Unknown);
}

#[test]
fn stale_session_or_binding_delta_cannot_confirm_a_rebound_send() {
    let ledger = ChatSendLedger::default();
    let pending = ledger
        .try_start_scoped(
            "tab-1",
            "exact payload",
            None,
            Some(SendLiveBinding {
                session_id: Some("session-new".into()),
                binding_nonce: Some("binding-new".into()),
            }),
        )
        .unwrap();
    ledger.finish(&pending.send_id, Ok(())).unwrap();

    assert!(ledger
        .confirm_delivered_user_turn_scoped(
            "tab-1",
            "session-new",
            Some("binding-old"),
            "exact payload",
        )
        .is_none());
    assert!(ledger
        .confirm_delivered_user_turn_scoped(
            "tab-1",
            "session-old",
            Some("binding-new"),
            "exact payload",
        )
        .is_none());
    assert_eq!(ledger.reconcile("tab-1")[0].state, ChatSendState::Unknown);

    let confirmed = ledger
        .confirm_delivered_user_turn_scoped(
            "tab-1",
            "session-new",
            Some("binding-new"),
            "exact payload",
        )
        .unwrap();
    assert_eq!(confirmed.state, ChatSendState::Sent);
}

#[test]
fn one_turn_confirms_at_most_one_send_oldest_first() {
    let ledger = ChatSendLedger::default();
    let first = ledger.try_start("tab-1", "same text", None).unwrap();
    let second = ledger.try_start("tab-1", "same text", None).unwrap();

    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "same text")
        .unwrap();
    assert_eq!(confirmed.send_id, first.send_id);
    assert!(ledger
        .state
        .lock()
        .by_id
        .get(&second.send_id)
        .is_some_and(|status| status.state == ChatSendState::Pending));

    let confirmed_second = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "same text")
        .unwrap();
    assert_eq!(confirmed_second.send_id, second.send_id);
    assert!(ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "same text")
        .is_none());
}

#[test]
fn rebind_window_send_is_not_sent_by_transport_classification_alone() {
    let ledger = ChatSendLedger::default();
    let pending = ledger
        .try_start("tab-1", "Reply with exactly: REBIND-1", None)
        .unwrap();
    let finished = ledger.finish(&pending.send_id, Ok(())).unwrap();
    assert_eq!(
        finished.state,
        ChatSendState::Unknown,
        "transport Ok is not attribution: sent requires a turn in the tab's own bound session"
    );
}

#[test]
fn transport_ok_for_rebind_window_send_defers_to_turn_evidence() {
    let ledger = ChatSendLedger::default();
    let pending = ledger
        .try_start("tab-1", "Reply with exactly: REBIND-2", None)
        .unwrap();
    let mut writes = Vec::new();
    let result = run_chat_send(
        &ledger,
        pending.clone(),
        "Reply with exactly: REBIND-2",
        |text| {
            writes.push(text.to_string());
            Ok(())
        },
    );
    assert_eq!(writes.len(), 1, "payload written exactly once");
    assert_eq!(
        result.state,
        ChatSendState::Unknown,
        "transport completion stays unknown until exact turn evidence"
    );
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "rebound-session", "Reply with exactly: REBIND-2")
        .expect("the attributable turn confirms the held send");
    assert_eq!(confirmed.send_id, pending.send_id);
    assert_eq!(confirmed.state, ChatSendState::Sent);
    assert_eq!(ledger.reconcile("tab-1")[0].state, ChatSendState::Sent);
}

#[test]
fn already_sent_deliveries_are_never_reconfirmed() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hello", None).unwrap();
    run_chat_send(&ledger, pending, "hello", |_| Ok(()));
    ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "hello")
        .unwrap();
    assert!(ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "hello")
        .is_none());
}

// W5-3: the send envelope's opaque client correlation token is echoed
// verbatim onto the committed user turn and nowhere else.

#[test]
fn client_token_echoes_verbatim_onto_committed_turn() {
    let ledger = ChatSendLedger::default();
    let token = "opt-1717-42";
    let pending = ledger.try_start("tab-1", "deploy", None).unwrap();
    let stamped = ledger
        .attach_client_token(&pending.send_id, token.to_string())
        .unwrap();
    // Rides on the accepted status too — the wire carries it from the start.
    assert_eq!(stamped.correlation_id.as_deref(), Some(token));
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "deploy")
        .expect("turn evidence confirms the pending send");
    assert_eq!(confirmed.state, ChatSendState::Sent);
    // Token IN == token OUT, verbatim, on the committed (confirmed) turn.
    assert_eq!(confirmed.correlation_id.as_deref(), Some(token));
}

#[test]
fn tokenless_send_emits_no_synthesized_token() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hello", None).unwrap();
    assert_eq!(pending.correlation_id, None);
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "hello")
        .unwrap();
    assert_eq!(confirmed.correlation_id, None);
    // Wire shape: the field is OMITTED, never emitted as a synthesized null.
    let json = serde_json::to_value(&confirmed).unwrap();
    assert!(json.get("correlationId").is_none());
}

#[test]
fn token_is_passthrough_only_and_never_gates_confirmation() {
    let ledger = ChatSendLedger::default();
    // Two sends, identical payload, DISTINCT tokens.
    let first = ledger.try_start("tab-1", "yes", None).unwrap();
    ledger
        .attach_client_token(&first.send_id, "tok-A".into())
        .unwrap();
    let second = ledger.try_start("tab-1", "yes", None).unwrap();
    ledger
        .attach_client_token(&second.send_id, "tok-B".into())
        .unwrap();
    // The confirm path carries NO token — selection is purely payload/binding,
    // oldest-first, exactly as before this change. The token never gates it.
    let c1 = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "yes")
        .unwrap();
    let c2 = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "yes")
        .unwrap();
    assert_eq!(c1.send_id, first.send_id);
    assert_eq!(c2.send_id, second.send_id);
    // Each committed turn echoes its OWN send's token — passthrough, not a
    // match key: the token rode along but never chose which send confirmed.
    assert_eq!(c1.correlation_id.as_deref(), Some("tok-A"));
    assert_eq!(c2.correlation_id.as_deref(), Some("tok-B"));
}

#[test]
fn emitted_wire_field_matches_chat_core_consumer_correlation_id() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab-1", "hi", None).unwrap();
    ledger
        .attach_client_token(&pending.send_id, "tok-Z".into())
        .unwrap();
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "hi")
        .unwrap();
    let json = serde_json::to_value(&confirmed).unwrap();
    // The serialized key is the contract the chat-core consumer (lane
    // ca6aebc5) reads as NormalizedChatMessage.correlationId.
    assert_eq!(json["correlationId"], serde_json::json!("tok-Z"));
}

#[test]
fn model_command_hold_is_queued_until_written_and_exact_turn_delivered() {
    let ledger = ChatSendLedger::default();
    let record: UserSendRecord = serde_json::from_value(serde_json::json!({
        "send_id": "send-1",
        "client_token": "token-1",
        "acceptance_sequence": 1,
        "ledger_sequence": 1,
        "authority": {
            "tab_id": "tab-1",
            "pty_id": "pty-1",
            "binding_nonce": "nonce-1",
            "pty_session_generation": 1,
            "provider_id": "provider",
            "session_id": "session-1",
            "observed_session_id": "session-1",
            "prelaunch": false
        },
        "owner_epoch": "epoch-1",
        "reservation": 1,
        "held_before_readiness": false,
        "state": "queued",
        "handoff_session": null,
        "typed_follow_up": null,
        "original_digest": "original",
        "original_bytes": 14,
        "prepared_digest": "prepared",
        "prepared_bytes": 14,
        "body_digest": null,
        "body_path": null,
        "body_bytes": 0
    }))
    .unwrap();
    let pending = ledger.admit_user(&record);
    let held = ledger
        .set_user_pending_reason(&pending.send_id, Some("awaiting_model_command"))
        .unwrap();
    assert_eq!(held.state, ChatSendState::Pending);
    assert_eq!(serde_json::to_value(&held).unwrap()["state"], "held");
    assert_eq!(
        held.pending_reason.as_deref(),
        Some("awaiting_model_command")
    );
    assert!(!ledger.user_payload_may_have_been_written(&held.send_id));
    assert!(ledger.begin_user_handoff(
        &record,
        "opaque payload",
        crate::service::chat_send_evidence::CapturedSendEvidence {
            anchor: Some(SendEvidenceAnchor {
                provider_id: "provider".into(),
                cwd: "/opaque".into(),
                session_id: "session-1".into(),
                window: EvidenceWindow::Jsonl {
                    transcript_offset: 100,
                },
                turn_normalization: Default::default(),
            }),
            live_binding: Some(SendLiveBinding {
                session_id: Some("session-1".into()),
                binding_nonce: Some("nonce-1".into()),
            }),
            ..Default::default()
        },
    ));
    let written = ledger.mark_user_written(&held.send_id).unwrap();
    assert_eq!(written.state, ChatSendState::Written);
    assert!(written.pending_reason.is_none());
    let messages = [crate::agent::chat_types::ChatMsg {
        id: "turn-1".into(),
        kind: "user".into(),
        content: "opaque payload".into(),
        ..Default::default()
    }];
    let provenance = LiveProvenance {
        source_session_id: "session-1".into(),
        source_binding_nonce: Some("nonce-1".into()),
        range: crate::service::tab_chat::tailer::SourceRange {
            start_offset: 100,
            offset: 200,
            incarnation: Some(crate::service::tab_chat::registry::SourceIncarnation {
                id: "incarnation-1".into(),
                absence_witness: None,
            }),
        },
        positions: vec![TurnPosition {
            record_start: Some(100),
            record_row: 0,
        }],
    };
    let confirm = |session, nonce, provenance: &LiveProvenance| {
        ledger.advance_live_evidence("tab-1", session, Some(nonce), provenance);
        ledger.confirm_owned_live_turns_with(
            "tab-1",
            (session, Some(nonce)),
            &messages[0],
            provenance.positions[0],
            provenance,
            |authority, session, nonce| {
                authority == &record.authority && session == "session-1" && nonce == "nonce-1"
            },
        )
    };
    assert!(confirm("foreign-session", "nonce-1", &provenance).is_empty());
    assert!(confirm("session-1", "foreign-nonce", &provenance).is_empty());
    let mut stale = provenance.clone();
    stale.range.start_offset = 0;
    stale.range.offset = 100;
    stale.positions[0].record_start = Some(99);
    assert!(confirm("session-1", "nonce-1", &stale).is_empty());
    assert_eq!(ledger.reconcile("tab-1")[0].state, ChatSendState::Written);
    let delivered = confirm("session-1", "nonce-1", &provenance);
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].state, ChatSendState::Sent);
    assert_eq!(delivered[0].send_id, held.send_id);
    assert!(confirm("session-1", "nonce-1", &provenance).is_empty());
}

#[test]
fn failed_user_admission_restore_is_retried_before_next_reservation() {
    let ledger = ChatSendLedger::default();
    let attempts = std::cell::Cell::new(0);
    let admit = || {
        ledger.restore_before_user_admission("tab-1", || {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                Err("opaque-transient-storage-error".into())
            } else {
                Ok(())
            }
        })?;
        Ok::<_, String>(ledger.reserve_identity())
    };

    assert_eq!(admit().unwrap_err(), "opaque-transient-storage-error");
    assert_eq!(attempts.get(), 1);
    let first = admit().unwrap();
    assert_eq!(attempts.get(), 2);
    let second = admit().unwrap();
    assert_eq!(attempts.get(), 2);
    assert_ne!(first.0, second.0);
    assert_eq!(second.1, first.1 + 1);
}

mod receipt_tests;

mod detached_append_tests;
