use super::*;

#[test]
fn confirmed_timeout_keeps_watching_but_unknown_resources_remain_fenced() {
    let failure = ProbeFailure::new("probe_timeout");
    assert_eq!(decide(None, &failure), Decision::Skip(failure.reason()));
    let mut uncertain = ProbeFailure::new("probe_wait_failed");
    uncertain.unconfirmed(ResourceKind::ProcessGroup);
    assert_eq!(decide(None, &uncertain), Decision::StopResources);
}

#[test]
fn timeout_keeps_watching_with_cleanup_diagnostics() {
    for resource in [
        ResourceKind::Child,
        ResourceKind::ProcessGroup,
        ResourceKind::Job,
        ResourceKind::Capture,
        ResourceKind::OverlappedIo,
    ] {
        let mut failure = ProbeFailure::new("probe_timeout");
        failure.unconfirmed(resource);
        let reason = failure.reason();
        assert_eq!(decide(None, &failure), Decision::Skip(reason.clone()));
        assert_eq!(decide(Some(&reason), &failure), Decision::Skip(reason));
    }
}

#[test]
fn probe_owner_capacity_stops_for_routed_lifecycle_failure() {
    assert_eq!(
        decide(None, &ProbeFailure::new("probe_owner_capacity")),
        Decision::StopResources
    );
}

fn capture() -> ProbeFailure {
    let mut failure = ProbeFailure::new("probe_capture_or_completion_failed");
    failure.processes_confirmed = true;
    failure.unconfirmed(ResourceKind::OverlappedIo);
    failure
}

#[test]
fn windows_capture_mapping_preserves_command_uncertainty_but_skips_probe() {
    let failure = capture();
    assert!(failure.is_unconfirmed());
    assert!(matches!(decide(None, &failure), Decision::Skip(_)));
}

#[test]
fn unconfirmed_child_or_missing_completion_never_downgrades() {
    let mut failure = capture();
    failure.processes_confirmed = false;
    assert_eq!(decide(None, &failure), Decision::StopResources);
    failure.processes_confirmed = true;
    for resource in [
        ResourceKind::Child,
        ResourceKind::Job,
        ResourceKind::ProcessGroup,
    ] {
        let mut failure = failure.clone();
        failure.unconfirmed(resource);
        assert_eq!(decide(None, &failure), Decision::StopResources);
    }
}

#[test]
fn third_consecutive_failure_stops_and_other_outcomes_reset_count() {
    let failure = capture();
    let Decision::Skip(first) = decide(None, &failure) else {
        panic!()
    };
    let Decision::Skip(second) = decide(Some(&first), &failure) else {
        panic!()
    };
    assert_eq!(decide(Some(&second), &failure), Decision::Stop);
    assert_eq!(decide(Some("unchanged"), &failure), decide(None, &failure));
    assert_eq!(
        decide(Some(&second), &ProbeFailure::new("probe_timeout")),
        Decision::Skip("probe_timeout".into())
    );
}
