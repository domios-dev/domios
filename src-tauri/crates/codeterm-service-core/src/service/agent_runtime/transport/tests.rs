use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding};
#[test]
fn uncertain_handoff_cannot_replay_after_recovery_and_only_own_live_evidence_delivers() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab", "opaque payload", None).unwrap();
    let bound = SendLiveBinding {
        session_id: Some("thread".into()),
        binding_nonce: Some("nonce".into()),
    };
    assert!(ledger.bind_rpc_send(&pending.send_id, bound.clone()));
    ledger.mark_rpc_uncertain(&pending.send_id);
    assert!(!ledger.bind_rpc_send(&pending.send_id, bound));
    assert!(ledger
        .confirm_rpc_send(
            "other-tab",
            "thread",
            "nonce",
            &pending.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab",
            "other-thread",
            "nonce",
            &pending.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab",
            "thread",
            "old-nonce",
            &pending.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab",
            "thread",
            "nonce",
            &pending.send_id,
            Some("provider-item")
        )
        .is_some());
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Sent
    );
    ledger.finish(&pending.send_id, Err("late pipe failure".into()));
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Sent
    );
}
#[test]
fn acceptance_is_written_and_never_fabricates_delivery() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab", "opaque payload", None).unwrap();
    assert!(ledger.bind_rpc_send(
        &pending.send_id,
        SendLiveBinding {
            session_id: Some("thread".into()),
            binding_nonce: Some("nonce".into())
        }
    ));
    ledger.mark_rpc_accepted(&pending.send_id);
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Written
    );
}

#[test]
fn runtime_turn_input_carries_images_once_and_preserves_marker_text() {
    let text =
        "describe <domios type=\"attachment\" kind=\"image\" path=\"C:/images/a&amp;b.jpg\" />";
    let input = super::turn_input(&format!("{text}\n{text}"));
    assert_eq!(input.text, format!("{text}\n{text}"));
    assert_eq!(input.attachments.len(), 1);
    assert_eq!(
        input.attachments[0].path.as_deref(),
        Some("C:/images/a&b.jpg")
    );
    assert_eq!(input.attachments[0].mime_type, "image/jpeg");
    assert_eq!(input.attachments[0].id, "attachment:C:/images/a&b.jpg");
    for text in [
        "plain input",
        "<domios type=\"attachment\" kind=\"file\" path=\"/work/file.txt\" />",
        "`<domios type=\"attachment\" kind=\"image\" path=\"/example.png\" />`",
    ] {
        let input = super::turn_input(text);
        assert_eq!(input.text, text);
        assert!(input.attachments.is_empty());
    }
}
