use super::*;

#[test]
fn rpc_ledger_admission_traces_the_existing_id_before_transport_and_exact_turn() {
    let ledger = ChatSendLedger::default();
    let events = crate::flow_trace::test_capture::capture(|| {
        let status = started(&ledger, "nonce-a");
        ledger.mark_rpc_accepted(&status.send_id).unwrap();
        assert!(ledger
            .confirm_rpc_send(
                "tab-a",
                "foreign",
                "nonce-a",
                &status.send_id,
                Some("provider-item")
            )
            .is_none());
        ledger
            .confirm_rpc_send(
                "tab-a",
                "thread-a",
                "nonce-a",
                &status.send_id,
                Some("provider-item"),
            )
            .unwrap();
    });
    assert_eq!(
        events
            .iter()
            .map(|event| event.stage.as_str())
            .collect::<Vec<_>>(),
        [
            "send.started",
            "send.written",
            "send.delivered",
            "transcript.append"
        ]
    );
    assert!(events
        .iter()
        .all(|event| event.trace_id == events[0].trace_id));
    assert!(events.iter().all(|event| event.tab == "tab-a"));
    assert!(ledger.status(&events[0].trace_id).is_some());
}

#[test]
fn rejected_ledger_admission_does_not_trace_a_started_flow() {
    let ledger = ChatSendLedger::default();
    let events = crate::flow_trace::test_capture::capture(|| {
        for _ in 0..super::super::MAX_IN_FLIGHT {
            ledger.try_start("tab-a", "payload", None).unwrap();
        }
        assert!(ledger.try_start("tab-a", "rejected", None).is_err());
    });
    assert_eq!(events.len(), super::super::MAX_IN_FLIGHT);
    assert!(events.iter().all(|event| event.stage == "send.started"));
}

fn started(ledger: &ChatSendLedger, nonce: &str) -> ChatSendStatus {
    let status = ledger.try_start("tab-a", "same payload", None).unwrap();
    assert!(ledger.bind_rpc_send(
        &status.send_id,
        SendLiveBinding {
            session_id: Some("thread-a".into()),
            binding_nonce: Some(nonce.into())
        }
    ));
    status
}

#[test]
fn codex_rpc_send_ledger_held_written_delivered_by_exact_client_id() {
    let ledger = ChatSendLedger::default();
    let status = started(&ledger, "nonce-a");
    assert_eq!(
        ledger.status(&status.send_id).unwrap().state,
        ChatSendState::Pending
    );
    assert_eq!(
        ledger.mark_rpc_accepted(&status.send_id).unwrap().state,
        ChatSendState::Written
    );
    assert_eq!(
        ledger.finish(&status.send_id, Ok(())).unwrap().state,
        ChatSendState::Written
    );
    assert_eq!(
        ledger
            .confirm_rpc_send(
                "tab-a",
                "thread-a",
                "nonce-a",
                &status.send_id,
                Some("provider-item")
            )
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    assert_eq!(
        ledger
            .finish(&status.send_id, Err("submit_unconfirmed: late".into()))
            .unwrap()
            .state,
        ChatSendState::Sent
    );
}

#[test]
fn codex_rpc_send_ledger_rejects_foreign_thread_tab_nonce_and_client_id() {
    let ledger = ChatSendLedger::default();
    let status = started(&ledger, "nonce-a");
    ledger.mark_rpc_accepted(&status.send_id);
    assert!(ledger
        .confirm_rpc_send(
            "tab-b",
            "thread-a",
            "nonce-a",
            &status.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-b",
            "nonce-a",
            &status.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-b",
            &status.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            "foreign-id",
            Some("provider-item")
        )
        .is_none());
    assert_eq!(
        ledger.status(&status.send_id).unwrap().state,
        ChatSendState::Written
    );
}

#[test]
fn codex_rpc_send_ledger_identical_text_and_jsonl_never_confirm_rpc() {
    let ledger = ChatSendLedger::default();
    let status = started(&ledger, "nonce-a");
    ledger.mark_rpc_accepted(&status.send_id);
    assert!(ledger
        .confirm_delivered_user_turns_scoped("tab-a", "thread-a", Some("nonce-a"), "same payload")
        .is_empty());
    assert!(ledger.evidence_candidates("tab-a").is_empty());
    assert!(ledger.deferred_first_turn_candidates("tab-a").is_empty());
    assert_eq!(
        ledger.status(&status.send_id).unwrap().state,
        ChatSendState::Written
    );
}

#[test]
fn codex_rpc_send_ledger_evidence_before_response_wins_and_one_item_confirms_once() {
    let ledger = ChatSendLedger::default();
    let first = started(&ledger, "nonce-a");
    let second = started(&ledger, "nonce-a");
    assert!(ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            &first.send_id,
            Some("provider-item")
        )
        .is_some());
    assert!(ledger.mark_rpc_accepted(&first.send_id).is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            &first.send_id,
            Some("provider-item")
        )
        .is_none());
    assert_eq!(
        ledger.status(&second.send_id).unwrap().state,
        ChatSendState::Pending
    );
}

#[test]
fn codex_rpc_send_ledger_uncertainty_keeps_late_exact_evidence_and_refuses_reclaim() {
    let ledger = ChatSendLedger::default();
    let status = started(&ledger, "nonce-a");
    assert!(ledger.has_rpc_handoff(&status.send_id));
    assert_eq!(
        ledger.mark_rpc_uncertain(&status.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    assert!(ledger.mark_rpc_accepted(&status.send_id).is_none());
    assert!(!ledger.bind_rpc_send(&status.send_id, SendLiveBinding::default()));
    assert!(ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            &status.send_id,
            Some("provider-item")
        )
        .is_some());
}

#[test]
fn codex_rpc_send_owned_user_finish_preserves_accepted_and_uncertain_states() {
    use crate::service::pending_delivery_payloads::user_sends::UserSendAuthority;
    let ledger = ChatSendLedger::default();
    let status = started(&ledger, "nonce-a");
    ledger.state.lock().owned.insert(
        status.send_id.clone(),
        (
            UserSendAuthority {
                tab_id: "tab-a".into(),
                pty_id: Some("pty-a".into()),
                binding_nonce: "nonce-a".into(),
                pty_session_generation: Some(1),
                provider_id: "fixture".into(),
                session_id: Some("thread-a".into()),
                observed_session_id: Some("thread-a".into()),
                prelaunch: false,
            },
            true,
        ),
    );
    ledger.mark_rpc_accepted(&status.send_id);
    assert_eq!(
        ledger.finish(&status.send_id, Ok(())).unwrap().state,
        ChatSendState::Written
    );
    ledger.mark_rpc_uncertain(&status.send_id);
    assert_eq!(
        ledger
            .finish(&status.send_id, Err("submit_unconfirmed: closed".into()))
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    assert!(ledger.has_rpc_handoff(&status.send_id));
}

#[test]
fn codex_rpc_send_ledger_sidecar_death_only_settles_its_launch() {
    let ledger = ChatSendLedger::default();
    let first = started(&ledger, "nonce-a");
    let second = started(&ledger, "nonce-b");
    ledger.mark_rpc_accepted(&first.send_id);
    ledger.mark_rpc_accepted(&second.send_id);
    let changed = ledger.rpc_uncertain_for_launch("tab-a", Some("nonce-a"));
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].send_id, first.send_id);
    assert_eq!(
        ledger.status(&second.send_id).unwrap().state,
        ChatSendState::Written
    );
}

#[test]
fn parked_user_delivery_accepted_rpc_does_not_block_later_held_message() {
    use crate::service::pending_delivery_payloads::user_sends::UserSendAuthority;
    let ledger = ChatSendLedger::default();
    let status = started(&ledger, "nonce-a");
    let authority = UserSendAuthority {
        tab_id: "tab-a".into(),
        pty_id: Some("pty-a".into()),
        binding_nonce: "nonce-a".into(),
        pty_session_generation: Some(1),
        provider_id: "fixture".into(),
        session_id: Some("thread-a".into()),
        observed_session_id: Some("thread-a".into()),
        prelaunch: false,
    };
    ledger
        .state
        .lock()
        .owned
        .insert(status.send_id.clone(), (authority.clone(), true));
    ledger.mark_rpc_accepted(&status.send_id);
    let temp = tempfile::tempdir().unwrap();
    let store =
        crate::service::pending_delivery_payloads::user_sends::UserSendStore::open(temp.path())
            .unwrap();
    let reservation = store
        .reserve(authority, None, || ledger.reserve_identity())
        .unwrap();
    let record = store
        .admit(&reservation, "next payload", "next payload", None)
        .unwrap();
    drop(reservation);
    ledger.admit_user(&record);
    assert!(ledger.begin_user_handoff(&record, "next payload", Default::default()));
    assert_eq!(
        ledger.mark_user_written(&record.send_id).unwrap().state,
        ChatSendState::Written
    );
    assert_eq!(
        ledger.status(&status.send_id).unwrap().state,
        ChatSendState::Written
    );
}

#[test]
fn runtime_items_claim_identical_sends_once_and_survive_durable_restore() {
    use crate::service::chat_send_transaction::{MatchedRow, SendRowClaim, SourceRowRef};
    use crate::service::pending_delivery_payloads::user_sends::UserSendStore;
    let ledger = ChatSendLedger::default();
    let first = started(&ledger, "nonce-a");
    let second = started(&ledger, "nonce-a");
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &first.send_id, Some(""))
        .is_none());
    let first = ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            &first.send_id,
            Some("user-1"),
        )
        .unwrap();
    assert!(ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            &second.send_id,
            Some("user-1")
        )
        .is_none());
    assert_eq!(
        ledger.status(&second.send_id).unwrap().state,
        ChatSendState::Pending
    );
    let second = ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            &second.send_id,
            Some("user-2"),
        )
        .unwrap();
    let claims: Vec<_> = [&first, &second]
        .into_iter()
        .map(|status| SendRowClaim::from_status(status).unwrap())
        .collect();
    for (status, item) in [(&first, "user-1"), (&second, "user-2")] {
        assert_eq!(
            status.matched_row,
            Some(MatchedRow {
                source: SourceRowRef::structured("thread-a", item).unwrap(),
                ordinal: 1
            })
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    store.record_source_claims("tab-a", &claims).unwrap();
    drop(store);
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let restored = ChatSendLedger::default();
    for claim in reopened.source_claims("tab-a").unwrap() {
        restored.restore_source_claim(&claim);
    }
    for status in [&first, &second] {
        let actual = restored.status(&status.send_id).unwrap();
        assert_eq!(actual.state, ChatSendState::Sent);
        assert_eq!(actual.matched_row, status.matched_row);
    }
    let third = started(&restored, "nonce-a");
    assert!(restored
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            &third.send_id,
            Some("user-1")
        )
        .is_none());
}

#[test]
fn runtime_send_evidence_without_item_identity_delivers_without_claiming_a_row() {
    let ledger = ChatSendLedger::default();
    let send = started(&ledger, "nonce-a");
    ledger.mark_rpc_accepted(&send.send_id).unwrap();
    assert!(ledger
        .confirm_rpc_send("tab-a", "foreign", "nonce-a", &send.send_id, None)
        .is_none());
    let delivered = ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &send.send_id, None)
        .unwrap();
    assert_eq!(delivered.state, ChatSendState::Sent);
    assert_eq!(delivered.matched_row, None);
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &send.send_id, None)
        .is_none());
    let next = started(&ledger, "nonce-a");
    assert!(ledger
        .confirm_rpc_send(
            "tab-a",
            "thread-a",
            "nonce-a",
            &next.send_id,
            Some("user-item")
        )
        .unwrap()
        .matched_row
        .is_some());
}
