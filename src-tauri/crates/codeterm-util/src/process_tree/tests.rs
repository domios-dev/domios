use super::terminate_and_wait;
use std::{cell::RefCell, io};

#[test]
fn termination_race_requires_observed_exit_of_every_pinned_process() {
    let calls = RefCell::new(Vec::new());
    let result = terminate_and_wait(
        &[1, 2],
        |process| {
            calls.borrow_mut().push(("terminate", *process));
            Err(io::Error::from_raw_os_error(5))
        },
        |process| {
            calls.borrow_mut().push(("exit", *process));
            Ok(())
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *calls.borrow(),
        [("terminate", 1), ("terminate", 2), ("exit", 1), ("exit", 2)]
    );
}

#[test]
fn termination_failure_without_observed_exit_keeps_the_original_error() {
    let waited = RefCell::new(Vec::new());
    let error = terminate_and_wait(
        &[1, 2],
        |_| Err(io::Error::from_raw_os_error(5)),
        |process| {
            waited.borrow_mut().push(*process);
            Err(io::Error::new(io::ErrorKind::TimedOut, "exit unobserved"))
        },
    )
    .unwrap_err();
    assert_eq!(error.raw_os_error(), Some(5));
    assert_eq!(*waited.borrow(), [1, 2]);
}

#[test]
fn successful_termination_without_observed_exit_is_not_completion() {
    let error = terminate_and_wait(
        &[1],
        |_| Ok(()),
        |_| Err(io::Error::new(io::ErrorKind::TimedOut, "exit unobserved")),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}
