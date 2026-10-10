use super::{AgentDeliveryRecovery, AgentDeliveryStore};

#[test]
fn restart_retires_claim_to_receipt_without_replaying_payload() {
    let dir = tempfile::tempdir().unwrap();
    let original = AgentDeliveryStore::open(dir.path());
    let tab_id = "tab-claimed";
    let delivery_id = original
        .enqueue(tab_id, "serialized payload", None)
        .unwrap();
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

    let restarted = AgentDeliveryStore::open(dir.path());
    assert!(restarted.pending_head(tab_id).unwrap().is_none());
    assert!(!restarted.has_pending(tab_id).unwrap());
    let receipts = restarted.pending_receipts(tab_id).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, delivery_id);
    assert_eq!(receipts[0].payload_kind, "worker_brief");
    assert_eq!(receipts[0].status, None);
    assert!(!super::payload_path(&restarted.tab(tab_id), &delivery_id)
        .unwrap()
        .exists());
}
