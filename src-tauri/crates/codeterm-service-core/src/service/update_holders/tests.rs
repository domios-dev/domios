use super::*;
use crate::multiplexer::types::ShellActivity;
use codeterm_types::pty::TimelineState;

#[test]
fn only_observed_idle_work_is_safe_to_close_for_update() {
    assert!(!holder_is_busy(false, None, ShellActivity::Idle));
    assert!(holder_is_busy(false, None, ShellActivity::Running));
    assert!(holder_is_busy(false, None, ShellActivity::Unknown));
    assert!(!holder_is_busy(
        true,
        Some(TimelineState::Idle),
        ShellActivity::Running
    ));
    assert!(holder_is_busy(
        true,
        Some(TimelineState::Working),
        ShellActivity::Idle
    ));
    assert!(holder_is_busy(
        true,
        Some(TimelineState::Question),
        ShellActivity::Idle
    ));
    assert!(holder_is_busy(true, None, ShellActivity::Idle));
}
