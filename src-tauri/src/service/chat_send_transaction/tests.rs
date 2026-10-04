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
            pty_id: "pty-1".into(),
            binding_nonce: "nonce-1".into(),
            pty_session_generation: Some(1),
            provider_id: "provider".into(),
            session_id: None,
            observed_session_id: Some("session-1".into()),
        };
        ledger.state.lock().owned.insert(pending.send_id.clone(), (authority, true));
        ledger.set_user_pending_reason(&pending.send_id, Some("readiness_changed_before_payload_write"));

        let status = ledger.finish(&pending.send_id, Err("zero_write: readiness changed".into())).unwrap();

        assert_eq!(status.state, ChatSendState::Pending);
        assert_eq!(status.error, None);
        assert_eq!(status.pending_reason.as_deref(), Some("readiness_changed_before_payload_write"));
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
            Err(tab_busy("still booting"))
                .or_else(|e: DeliveryError| e.into_surfaced(SendOrigin::User))
        });
        let confirmed = ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "deploy")
            .expect("turn evidence confirms the pending user send once the PTY links");
        assert_eq!(confirmed.send_id, pending.send_id);
        assert_eq!(confirmed.state, ChatSendState::Sent);
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
    fn serialized_states_separate_acceptance_uncertainty_and_confirmation() {
        assert_eq!(
            serde_json::to_string(&ChatSendState::Pending).unwrap(),
            "\"accepted\""
        );
        assert_eq!(
            serde_json::to_string(&ChatSendState::Unknown).unwrap(),
            "\"unknown\""
        );
        assert_eq!(
            serde_json::to_string(&ChatSendState::Sent).unwrap(),
            "\"confirmed\""
        );
        assert_eq!(
            serde_json::to_string(&ChatSendState::Failed).unwrap(),
            "\"failed\""
        );
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

        assert_eq!(ledger.reconcile("tab-1"), vec![failed]);
        assert_eq!(second.state, ChatSendState::Pending);
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
    fn provider_normalized_user_turn_confirms_chat_send() {
        let ledger = ChatSendLedger::default();
        let normalization = crate::agent::provider::TurnEvidenceNormalization {
            unwrap_pasted_content_wrapper: true,
            collapse_horizontal_whitespace: true,
            drop_supplementary_characters: false,
        };
        let pending = ledger
            .try_start_scoped_with_normalization(
                "tab-1",
                "name\tvalue\n\talpha\tbeta",
                None,
                Some(SendLiveBinding {
                    session_id: Some("session-1".into()),
                    binding_nonce: Some("binding-1".into()),
                }),
                normalization,
            )
            .unwrap();
        ledger.finish(&pending.send_id, Ok(())).unwrap();

        let confirmed = ledger
            .confirm_delivered_user_turn_scoped(
                "tab-1",
                "session-1",
                Some("binding-1"),
                "<pasted_content id=\"4\">\nname      value\n        alpha    beta\n</pasted_content id=\"4\">",
            )
            .expect("the provider-normalized exact user turn confirms its send");

        assert_eq!(confirmed.send_id, pending.send_id);
        assert_eq!(confirmed.state, ChatSendState::Sent);
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
