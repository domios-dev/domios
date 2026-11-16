use super::*;
use crate::process::ProcessExit;

#[test]
fn graceful_exit_never_forces_or_kills() {
    let mut model = TerminationModel::new(42);
    assert!(!model.may_discard_profile());
    assert_eq!(model.observed_exit(), TerminationAction::ConfirmExit);
    assert!(model.may_discard_profile());
    assert_eq!(model.graceful_deadline(), TerminationAction::Wait);
    assert_eq!(model.force_deadline(Some(42)), TerminationAction::Wait);
}

#[test]
fn stopped_browser_escalates_once_only_to_its_verified_launch_pid() {
    let mut model = TerminationModel::new(42);
    assert_eq!(model.graceful_deadline(), TerminationAction::ForceTerminate);
    assert!(!model.may_discard_profile());
    assert_eq!(
        model.force_deadline(Some(42)),
        TerminationAction::KillOwnedPid(42)
    );
    assert!(!model.may_discard_profile());
    assert_eq!(model.force_deadline(Some(42)), TerminationAction::Wait);
    assert_eq!(model.observed_exit(), TerminationAction::ConfirmExit);
    assert!(model.may_discard_profile());
}

#[test]
fn unverified_or_different_pid_never_receives_sigkill() {
    for verified in [None, Some(43)] {
        let mut model = TerminationModel::new(42);
        model.graceful_deadline();
        assert_eq!(model.force_deadline(verified), TerminationAction::Wait);
        assert_eq!(model.force_deadline(Some(42)), TerminationAction::Wait);
        assert!(!model.may_discard_profile());
    }
    for pid in [0, 1] {
        let mut model = TerminationModel::new(pid);
        model.graceful_deadline();
        assert_eq!(model.force_deadline(Some(pid)), TerminationAction::Wait);
    }
}

#[test]
fn force_termination_can_confirm_exit_before_the_sigkill_stage() {
    let mut model = TerminationModel::new(42);
    model.graceful_deadline();
    assert_eq!(model.observed_exit(), TerminationAction::ConfirmExit);
    assert_eq!(model.force_deadline(Some(42)), TerminationAction::Wait);
    assert!(model.may_discard_profile());
}

#[test]
fn unconfirmed_close_is_a_typed_failure_and_late_exit_permits_cleanup() {
    let mut model = TerminationModel::new(42);
    model.graceful_deadline();
    model.force_deadline(Some(42));
    assert_eq!(
        close_result(Some(CloseOutcome::Killed(None))),
        Err(TerminationFailure::ExitUnconfirmed)
    );
    assert!(!model.may_discard_profile());
    model.observed_exit();
    assert!(model.may_discard_profile());
    assert!(close_result(Some(CloseOutcome::Killed(Some(ProcessExit { code: None })))).is_ok());
}

#[test]
fn close_result_accepts_confirmed_exits_attached_sessions_and_shared_leases() {
    let exited = ProcessExit { code: Some(0) };
    for outcome in [
        None,
        Some(CloseOutcome::AlreadyExited(exited)),
        Some(CloseOutcome::Graceful(exited)),
        Some(CloseOutcome::Killed(Some(exited))),
    ] {
        assert_eq!(close_result(outcome), Ok(()));
    }
    let failure = close_result(Some(CloseOutcome::Killed(None))).unwrap_err();
    assert_eq!(failure.code(), "browser_termination_unconfirmed");
    assert!(failure.to_string().contains(failure.code()));
}
