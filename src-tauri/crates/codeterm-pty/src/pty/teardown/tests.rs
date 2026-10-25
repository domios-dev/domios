use super::*;

#[test]
fn darwin_group_permission_error_requires_confirmed_leader_exit() {
    for (errno, exited_group_errno, exited, accepted) in [
        (1, Some(1), true, true),
        (1, Some(1), false, false),
        (1, None, true, false),
        (13, Some(1), true, false),
        (3, Some(1), true, false),
    ] {
        let result = classify_tree_kill(
            Err(std::io::Error::from_raw_os_error(errno)),
            exited_group_errno,
            || exited,
        );
        assert_eq!(result.is_ok(), accepted);
        if let Err(error) = result {
            assert_eq!(error.raw_os_error(), Some(errno));
        }
    }
    assert!(classify_tree_kill(Ok(()), Some(1), || panic!("signal succeeded")).is_ok());
    let error = classify_tree_kill(
        Err(std::io::Error::other("unknown signal failure")),
        None,
        || panic!("no Darwin errno"),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "unknown signal failure");
}

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
    assert!(signal
        .wait_for_boundary(Some(Instant::now()), false)
        .is_err());
    signal.mark_child_exited();
    assert!(signal
        .wait_for_boundary(Some(Instant::now()), false)
        .is_ok());
}

#[test]
fn exit_boundary_keeps_failures_and_full_cleanup_deadlines_honest() {
    let signal = PtyTeardownSignal::default();
    signal.state.lock().console_closed = false;
    signal.mark_reader_started();
    signal.mark_child_exited();
    assert!(signal
        .wait_for_boundary(Some(Instant::now()), false)
        .is_ok());
    assert!(signal.wait_for_completion(Instant::now()).is_err());
    signal.mark_reader_finished();
    assert!(signal.wait_for_completion(Instant::now()).is_err());
    signal.state.lock().console_closed = true;
    assert!(signal.wait_for_completion(Instant::now()).is_ok());
    signal.mark_child_failed("termination failed".into());
    assert_eq!(
        signal
            .wait_for_boundary(Some(Instant::now()), false)
            .unwrap_err(),
        "termination failed"
    );
}

#[test]
fn restart_escalates_expired_grace_and_waits_for_exit() {
    let events = std::cell::RefCell::new(Vec::new());
    restart_after_grace(
        false,
        || {
            events.borrow_mut().push("terminate tree");
            Ok(())
        },
        || {
            events.borrow_mut().push("observe exit");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*events.borrow(), ["terminate tree", "observe exit"]);
}

#[test]
fn restart_accepts_observed_exit_with_cleanup_pending() {
    restart_after_grace(true, || panic!("already exited"), || Ok(())).unwrap();
}

#[test]
fn restart_refuses_failed_termination_without_claiming_exit() {
    let result = restart_after_grace(
        false,
        || Err("access denied".into()),
        || panic!("termination failed"),
    );
    assert_eq!(result, Err("access denied".into()));
}

#[test]
fn restart_retains_process_handle_wait_failure() {
    assert_eq!(
        restart_after_grace(false, || Ok(()), || Err("handle wait failed".into())),
        Err("handle wait failed".into())
    );
}

#[test]
fn restart_confirms_native_exit_with_reader_cleanup_pending() {
    #[cfg(windows)]
    let _guard = crate::pty::windows_test_support::exclusive_conpty_test();
    let mut manager = crate::pty::manager::PtyManager::new();
    let built = crate::pty::manager::build_session(
        manager.spawn_gate(),
        None,
        None,
        80,
        24,
        std::collections::HashMap::new(),
    )
    .expect("spawn fixture PTY");
    let (pty_id, session) = manager.register_session(None, built);
    let reader = session.reader.lock().unwrap().take();
    session.teardown.mark_reader_started();
    manager
        .begin_kill_tree(&pty_id)
        .unwrap()
        .wait_for_restart(std::time::Duration::ZERO)
        .expect("restart must confirm native exit without waiting for the reader");
    assert!(session
        .teardown
        .wait_for_completion(Instant::now())
        .is_err());
    drop(reader);
    session.teardown.mark_reader_finished();
}
