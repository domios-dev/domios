use super::*;

#[test]
fn termination_requires_observed_process_exit_but_not_console_or_reader_cleanup() {
    for resources_closed in [false, true] {
        assert!(!teardown_boundary_reached(false, resources_closed, false));
        assert!(!teardown_boundary_reached(false, resources_closed, true));
        assert!(teardown_boundary_reached(true, resources_closed, false));
        assert_eq!(
            teardown_boundary_reached(true, resources_closed, true),
            resources_closed
        );
    }
}

#[test]
fn live_tree_refuses_termination_even_when_cleanup_has_finished() {
    let signal = PtyTeardownSignal::default();
    assert!(signal.wait_for_boundary(Instant::now(), false).is_err());
    signal.mark_child_exited();
    assert!(signal.wait_for_boundary(Instant::now(), false).is_ok());
}

#[test]
fn exit_boundary_keeps_failures_and_full_cleanup_deadlines_honest() {
    let signal = PtyTeardownSignal::default();
    signal.state.lock().console_closed = false;
    signal.mark_reader_started();
    signal.mark_child_exited();
    assert!(signal.wait_for_boundary(Instant::now(), false).is_ok());
    assert!(signal.wait_for_completion(Instant::now()).is_err());
    signal.mark_reader_finished();
    assert!(signal.wait_for_completion(Instant::now()).is_err());
    signal.state.lock().console_closed = true;
    assert!(signal.wait_for_completion(Instant::now()).is_ok());
    signal.mark_child_failed("termination failed".into());
    assert_eq!(
        signal.wait_for_boundary(Instant::now(), false).unwrap_err(),
        "termination failed"
    );
}
