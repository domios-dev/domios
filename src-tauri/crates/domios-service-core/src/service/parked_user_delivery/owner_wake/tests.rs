use super::*;

#[test]
fn readiness_during_failed_drain_is_not_consumed() {
    let failed = Err("authority rebind failed".into());
    let boundary = OwnerWakeBoundary::after_drain(4, 2, &failed);
    assert_eq!(boundary.refusal_generation(), Some(2));
    assert!(!boundary.changed(4, 2));
    assert!(boundary.changed(5, 3));

    let drained = OwnerWakeBoundary::after_drain(5, 3, &Ok(UserDrain::Idle));
    assert!(!drained.changed(5, 3));
}

#[test]
fn housekeeping_does_not_retry_failed_drain() {
    let boundary = OwnerWakeBoundary::after_drain(4, 2, &Err("persist failed".into()));
    assert!(!boundary.changed(5, 2));
    assert!(boundary.changed(6, 3));
}

#[test]
fn zero_write_refusal_keeps_its_submit_boundary() {
    let boundary = OwnerWakeBoundary::after_drain(
        4,
        2,
        &Ok(UserDrain::ZeroWriteRefusal {
            dispatch_generation: 3,
        }),
    );
    assert_eq!(boundary.refusal_generation(), Some(3));
    assert!(!boundary.changed(5, 3));
    assert!(boundary.changed(6, 4));
}

#[test]
fn idle_drain_observes_wakes_during_drain() {
    let boundary = OwnerWakeBoundary::after_drain(4, 2, &Ok(UserDrain::Idle));
    assert_eq!(boundary.refusal_generation(), None);
    assert!(!boundary.changed(4, 2));
    assert!(boundary.changed(5, 2));
}
