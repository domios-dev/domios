use super::*;

#[test]
fn receipt_watch_capacity_is_reclaimed_when_a_watch_ends() {
    let slots = AnswerWatchSlots::default();
    let mut watches = (0..MAX_ANSWER_WATCHES)
        .map(|_| Arc::clone(&slots.0).try_acquire_owned().unwrap())
        .collect::<Vec<_>>();
    assert!(slots.0.try_acquire().is_err());
    drop(watches.pop());
    assert!(slots.0.try_acquire().is_ok());
}

#[test]
fn only_own_tab_delivery_evidence_can_confirm_the_answer() {
    let ledger = crate::service::chat_send_transaction::ChatSendLedger::default();
    let pending = ledger.try_start("asking-tab", "opaque", None).unwrap();
    assert!(exact_outcome(Some(pending.clone()), "asking-tab").is_none());
    let delivered = ledger.confirm_send(&pending.send_id).unwrap();
    assert!(exact_outcome(Some(delivered.clone()), "unrelated-tab").is_none());
    assert!(matches!(
        exact_outcome(Some(delivered), "asking-tab"),
        Some(AnswerHandoff::Confirmed)
    ));
}
