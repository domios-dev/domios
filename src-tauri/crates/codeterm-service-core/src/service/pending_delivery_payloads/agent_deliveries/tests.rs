use super::{AgentDeliveryRecovery, AgentDeliveryStore};

#[test]
fn completed_agent_delivery_ids_remain_inspectable_without_replay() {
    use crate::service::chat_send_transaction::ChatSendState;
    let dir = tempfile::tempdir().unwrap();
    let store = AgentDeliveryStore::open(dir.path());
    let held = store.enqueue("own-tab", "never written", None).unwrap();
    assert!(store.discard_expired("own-tab", &held).unwrap());
    let written = store.enqueue("own-tab", "opaque payload", None).unwrap();
    assert!(store
        .claim_with_recovery(
            "own-tab",
            &written,
            AgentDeliveryRecovery {
                payload_kind: "agent_message".into(),
                worker_brief: false,
                report: false,
                observer: None,
                exact_text: "opaque payload".into(),
                anchor: None,
            }
        )
        .unwrap());
    assert!(store.complete_written("own-tab", &written).unwrap());
    let restarted = AgentDeliveryStore::open(dir.path());
    let failure = restarted.inspect(&held).unwrap().unwrap();
    assert_eq!(failure.state, ChatSendState::Failed);
    assert_eq!(failure.error.as_deref(), Some("launch_ended_before_write"));
    assert_eq!(
        restarted.inspect(&written).unwrap().unwrap().state,
        ChatSendState::Unknown
    );
    assert!(restarted.pending_head("own-tab").unwrap().is_none());
    assert!(restarted
        .inspect(&uuid::Uuid::new_v4().to_string())
        .unwrap()
        .is_none());
}

#[test]
fn discovery_skips_receipt_only_directories_and_keeps_pending_deliveries() {
    let dir = tempfile::tempdir().unwrap();
    let store = AgentDeliveryStore::open(dir.path());
    for ordinal in 0..672 {
        let tab = store.tab(&format!("retired-{ordinal}"));
        std::fs::create_dir_all(tab.dir.join("receipts/pending")).unwrap();
    }
    store.enqueue("pending-tab", "payload", None).unwrap();

    let tabs = store
        .discover_tab_ids(|_, error| panic!("unexpected discovery error: {error}"))
        .unwrap();
    assert_eq!(tabs, vec!["pending-tab"]);
    assert!(store.has_pending("pending-tab").unwrap());
}

#[test]
fn discovery_continues_past_corrupt_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let store = AgentDeliveryStore::open(dir.path());
    for ordinal in 0..10 {
        let tab = store.tab(&format!("corrupt-{ordinal}"));
        std::fs::create_dir_all(&tab.dir).unwrap();
        std::fs::write(tab.dir.join("index.json"), b"invalid json").unwrap();
    }
    store.enqueue("pending-tab", "payload", None).unwrap();

    let mut reports = Vec::new();
    let tabs = store
        .discover_tab_ids(|count, error| reports.push((count, error.to_owned())))
        .unwrap();
    assert_eq!(tabs, vec!["pending-tab"]);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].0, 10);
}

#[test]
fn restart_retires_claim_to_receipt_without_replaying_payload() {
    let dir = tempfile::tempdir().unwrap();
    let original = AgentDeliveryStore::open(dir.path());
    let tab_id = "tab-claimed";
    let delivery_id = original
        .enqueue(tab_id, "serialized payload", None)
        .unwrap();
    let held = original.inspect(&delivery_id).unwrap().unwrap();
    assert_eq!(
        held.state,
        crate::service::chat_send_transaction::ChatSendState::Pending
    );
    assert_eq!(held.tab_id, tab_id);
    assert_eq!(held.send_id, delivery_id);
    assert!(original
        .claim_with_recovery(
            tab_id,
            &delivery_id,
            AgentDeliveryRecovery {
                payload_kind: "worker_brief".into(),
                worker_brief: true,
                report: false,
                observer: None,
                exact_text: "the exact turn".into(),
                anchor: None,
            },
        )
        .unwrap());
    assert_eq!(
        original.inspect(&delivery_id).unwrap().unwrap().state,
        crate::service::chat_send_transaction::ChatSendState::Unknown
    );

    let restarted = AgentDeliveryStore::open(dir.path());
    assert!(restarted.pending_head(tab_id).unwrap().is_none());
    assert!(!restarted.has_pending(tab_id).unwrap());
    let receipts = restarted.pending_receipts(tab_id).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, delivery_id);
    assert_eq!(receipts[0].payload_kind, "worker_brief");
    assert_eq!(receipts[0].status, None);
    assert_eq!(
        restarted.inspect(&delivery_id).unwrap().unwrap().state,
        crate::service::chat_send_transaction::ChatSendState::Unknown
    );
    assert!(!super::payload_path(&restarted.tab(tab_id), &delivery_id)
        .unwrap()
        .exists());
    let mut receipt = receipts[0].clone();
    receipt.status = Some(super::AgentDeliveryReceiptStatus::SubmitUnconfirmed);
    restarted.complete_receipt(&mut receipt).unwrap();
    assert_eq!(
        restarted.inspect(&delivery_id).unwrap().unwrap().state,
        crate::service::chat_send_transaction::ChatSendState::Unknown
    );
    receipt.status = Some(super::AgentDeliveryReceiptStatus::Delivered);
    restarted.complete_receipt(&mut receipt).unwrap();
    assert_eq!(
        restarted.inspect(&delivery_id).unwrap().unwrap().state,
        crate::service::chat_send_transaction::ChatSendState::Sent
    );
    assert!(!restarted.has_pending(tab_id).unwrap());
    assert!(restarted.inspect("not-a-delivery").unwrap().is_none());
}
