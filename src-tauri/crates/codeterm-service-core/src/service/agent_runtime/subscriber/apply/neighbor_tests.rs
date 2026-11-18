use crate::service::chat_send_transaction::ChatSendLedger;
use crate::service::messenger::InputProvenance;
use crate::service::pending_delivery_payloads::user_sends::{UserSendAuthority, UserSendStore};

#[test]
fn neighbor_turn_evidence_uses_host_envelope_identity_after_system_send_projection() {
    let directory = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(directory.path()).unwrap();
    let ledger = ChatSendLedger::default();
    let message = "b".repeat(32);
    let cases = [
        (
            format!("runtime-envelope:neighbor:opaque_sender_with_underscores_{message}"),
            InputProvenance::Neighbor,
            Some(message.clone()),
        ),
        (
            format!("runtime-envelope:neighbor:{message}"),
            InputProvenance::Local,
            None,
        ),
        (
            "runtime-envelope:neighbor:opaque_malformed".into(),
            InputProvenance::Neighbor,
            None,
        ),
    ];
    for (index, (token, provenance, expected)) in cases.into_iter().enumerate() {
        let authority = UserSendAuthority {
            tab_id: format!("tab-{index}"),
            pty_id: None,
            binding_nonce: "nonce".into(),
            pty_session_generation: None,
            provider_id: "provider".into(),
            session_id: Some("thread".into()),
            observed_session_id: Some("thread".into()),
            prelaunch: false,
        };
        let reservation = store
            .reserve(authority, Some(token), || ledger.reserve_identity())
            .unwrap();
        let record = store
            .admit_runtime_envelope(&reservation, "opaque", None, provenance)
            .unwrap();
        let status = ledger.admit_user(&record);
        drop(reservation);
        let bubble = ledger
            .attach_system_send(&status.send_id, "opaque", 1)
            .unwrap();
        assert_eq!(
            bubble.correlation_id.as_deref(),
            Some(status.send_id.as_str())
        );
        assert_eq!(ledger.neighbor_message_for_send(&status.send_id), expected);
        let restored = ChatSendLedger::default();
        restored.restore_user_record(&record);
        assert_eq!(
            restored.neighbor_message_for_send(&status.send_id),
            expected
        );
    }
}
