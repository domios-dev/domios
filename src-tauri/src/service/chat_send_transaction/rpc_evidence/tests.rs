use super::*;

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
            .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &status.send_id)
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
        .confirm_rpc_send("tab-b", "thread-a", "nonce-a", &status.send_id)
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-b", "nonce-a", &status.send_id)
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-b", &status.send_id)
        .is_none());
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", "foreign-id")
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
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &first.send_id)
        .is_some());
    assert!(ledger.mark_rpc_accepted(&first.send_id).is_none());
    assert!(ledger
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &first.send_id)
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
        .confirm_rpc_send("tab-a", "thread-a", "nonce-a", &status.send_id)
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
