use super::*;
use crate::multiplexer::types::PendingDeliveryChannel;
#[test]
fn startup_uses_the_existing_claim_and_never_replays_an_attempt_after_resume() {
    let mut record = PendingDeliveryRecord::new(
        "opaque-digest".into(),
        PendingDeliveryChannel::InteractiveInput,
    )
    .with_send_id("own-send".into());
    assert!(claimable(&record));
    record
        .begin_attempt(crate::service::launch_delivery::next_attempt())
        .unwrap();
    assert!(!claimable(&record));
    record.mark_unconfirmed();
    let restored: PendingDeliveryRecord =
        serde_json::from_value(serde_json::to_value(&record).unwrap()).unwrap();
    assert!(!claimable(&restored));
    assert_eq!(restored.send_id(), Some("own-send"));
    let mut accepted = restored;
    accepted.confirm_strong().unwrap();
    assert!(!claimable(&accepted));
}
#[test]
fn preclaimed_runtime_id_cannot_admit_a_second_payload() {
    let ledger = crate::service::chat_send_transaction::ChatSendLedger::default();
    let id = uuid::Uuid::new_v4().to_string();
    let first = ledger.start_runtime_claim("tab", "opaque", &id).unwrap();
    assert_eq!(first.send_id, id);
    assert!(ledger.start_runtime_claim("tab", "opaque", &id).is_err());
    assert!(ledger
        .start_runtime_claim("other", "different", &id)
        .is_err());
}

#[test]
fn readiness_preserves_pending_submission_but_rejects_a_prewrite_failure() {
    use crate::service::delivery_error::DeliveryError;
    assert!(readiness_after_submit(Ok(())).is_ok());
    assert!(
        readiness_after_submit(Err(DeliveryError::SubmitUnconfirmed {
            tab: "own-tab".into(),
            detail: "opaque pending evidence".into(),
        }))
        .is_ok()
    );
    assert!(matches!(
        readiness_after_submit(Err(DeliveryError::TabBusyRefused {
            tab:"own-tab".into(),detail:"opaque refusal before write".into(),
        })),
        Err(RuntimeFailure::Refused(message)) if message.starts_with("tab_busy_refused:")
    ));
}
