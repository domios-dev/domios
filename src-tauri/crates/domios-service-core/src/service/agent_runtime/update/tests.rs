use super::*;

#[test]
fn update_handoff_waits_for_turn_and_recovers_only_once() {
    assert_eq!(decide(true, false, false, false, false), Decision::Restart);
    assert_eq!(decide(true, true, false, false, false), Decision::Wait);
    assert_eq!(decide(true, false, false, true, false), Decision::Observe);
    assert_eq!(decide(true, false, true, true, false), Decision::Restart);
    assert_eq!(decide(true, false, false, true, true), Decision::Observe);
    assert_eq!(decide(true, false, true, true, true), Decision::Stop);
    assert_eq!(decide(false, false, true, false, false), Decision::Stop);
    assert_eq!(decide(true, true, true, true, false), Decision::Wait);
}

#[test]
fn runtime_version_mismatch_uses_only_reported_version() {
    assert!(!version_mismatch(None, "2.0"));
    assert!(!version_mismatch(Some("2.0"), "2.0"));
    assert!(version_mismatch(Some("1.0"), "2.0"));
}

#[test]
fn recoverable_failure_is_loading_until_the_single_recovery_is_exhausted() {
    let mut state = super::super::model::RuntimeState::unloaded(Some("own-thread".into()));
    state.degrade(
        0,
        &RuntimeFailure::SpawnFailed {
            reason: "fixture".into(),
        },
    );
    let mut hidden = state.snapshot();
    mask_recoverable_failure(&mut hidden, true);
    assert_eq!(hidden.state, "loading");
    assert_eq!(hidden.reason, None);
    let mut visible = state.snapshot();
    mask_recoverable_failure(&mut visible, false);
    assert_eq!(visible.state, "degraded");
    assert!(visible.reason.is_some());
}
