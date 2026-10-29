use super::*;

#[test]
fn cancelled_queued_actions_cannot_mutate_a_newer_surface() {
    let phase = Arc::new(AtomicU8::new(QUEUED));
    drop(CancelQueued(phase.clone()));
    assert_eq!(phase.load(Ordering::Acquire), CANCELLED);
    assert!(phase
        .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
        .is_err());
}

#[test]
fn cancellation_does_not_claim_an_already_running_action_was_prevented() {
    let phase = Arc::new(AtomicU8::new(RUNNING));
    drop(CancelQueued(phase.clone()));
    assert_eq!(phase.load(Ordering::Acquire), RUNNING);
}
