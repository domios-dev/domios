use super::*;

#[test]
fn admission_refuses_capacity_and_recovers_only_when_operations_exit() {
    let admission = Arc::new(Admission::default());
    let mut held = (0..MAX_OPERATIONS)
        .map(|_| admission.enter(None).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(admission.enter(None), Err(VmError::AdmissionBusy)));
    held[0].context.revoke();
    assert!(matches!(admission.enter(None), Err(VmError::AdmissionBusy)));
    held.pop();
    assert!(admission.enter(None).is_ok());
}

#[test]
fn reload_revokes_existing_operations_and_refuses_new_admission() {
    let admission = Arc::new(Admission::default());
    let first = admission.enter(None).unwrap();
    let pause = admission.pause();
    let second_pause = admission.pause();
    assert!(matches!(first.context.check(), Err(VmError::CallCancelled)));
    assert!(matches!(admission.enter(None), Err(VmError::AdmissionBusy)));
    drop(pause);
    assert!(matches!(admission.enter(None), Err(VmError::AdmissionBusy)));
    drop(second_pause);
    assert!(admission.enter(None).is_ok());
    assert!(matches!(first.context.check(), Err(VmError::CallCancelled)));
}

#[test]
fn completing_one_call_does_not_cancel_its_callers_shared_token() {
    let admission = Arc::new(Admission::default());
    let cancellation = ConversionCancellation::default();
    let operation = admission
        .enter(Some((
            cancellation.clone(),
            Instant::now() + OPERATION_BOUND,
        )))
        .unwrap();
    drop(operation);
    assert!(!cancellation.is_cancelled());
}

#[test]
fn expired_or_cancelled_calls_never_take_admission() {
    let admission = Arc::new(Admission::default());
    let cancellation = ConversionCancellation::default();
    assert!(matches!(
        admission.enter(Some((cancellation.clone(), Instant::now()))),
        Err(VmError::AsyncJobTimedOut)
    ));
    cancellation.cancel();
    assert!(matches!(
        admission.enter(Some((cancellation, Instant::now() + OPERATION_BOUND))),
        Err(VmError::CallCancelled)
    ));
    assert!(admission.state.lock().active.is_empty());
}
