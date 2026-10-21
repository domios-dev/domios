use super::*;
use crate::semantic_marker::SemanticMarkerParser;

fn feed(model: &mut ShellCommandStateModel, input: &[u8], nonce: Option<&str>) {
    for event in SemanticMarkerParser::new().feed(input) {
        model.apply(&event, nonce);
    }
}

#[test]
fn shell_command_state_unknown_without_marks() {
    let mut model = ShellCommandStateModel::default();
    feed(&mut model, b"compiling... done!", Some("own"));
    assert_eq!(model.state(), ShellCommandState::default());
}

#[test]
fn shell_command_state_tracker_streaming_and_nonce_wiring() {
    let mut tracker = crate::command_detection::ShellCapabilityTracker::new();
    tracker.feed("pty", Some("own".into()), b"\x1b]633;E;;own\x07\x1b]633;");
    assert_eq!(tracker.command_state("pty"), ShellCommandState::default());
    tracker.feed("pty", Some("own".into()), b"C\x07");
    assert_eq!(
        tracker.command_state("pty").activity,
        ShellActivity::Running
    );
    tracker.feed("pty", Some("own".into()), b"\x1b]633;D;4\x07");
    assert_eq!(
        tracker.command_state("pty"),
        ShellCommandState {
            activity: ShellActivity::Idle,
            last_exit_code: Some(4)
        }
    );
    tracker.feed(
        "pty",
        Some("own".into()),
        b"\x1b]633;E;;foreign\x07\x1b]633;C\x07\x1b]633;D;0\x07",
    );
    assert_eq!(
        tracker.command_state("pty"),
        ShellCommandState {
            activity: ShellActivity::Idle,
            last_exit_code: Some(4)
        }
    );
    tracker.remove("pty");
    assert_eq!(tracker.command_state("pty"), ShellCommandState::default());
}

#[test]
fn shell_command_state_prompt_start_finish_and_last_exit() {
    let mut model = ShellCommandStateModel::default();
    feed(&mut model, b"\x1b]633;A\x07", Some("own"));
    assert_eq!(model.state().activity, ShellActivity::Idle);
    assert_eq!(model.state().last_exit_code, None);
    for code in [0, 9] {
        feed(
            &mut model,
            b"\x1b]633;E;cmd;own\x07\x1b]633;C\x07",
            Some("own"),
        );
        assert_eq!(model.state().activity, ShellActivity::Running);
        feed(
            &mut model,
            format!("\x1b]633;D;{code}\x07\x1b]633;A\x07").as_bytes(),
            Some("own"),
        );
        assert_eq!(
            model.state(),
            ShellCommandState {
                activity: ShellActivity::Idle,
                last_exit_code: Some(code)
            }
        );
    }
}

#[test]
fn shell_command_state_rejects_wrong_or_missing_nonce() {
    let mut model = ShellCommandStateModel::default();
    feed(
        &mut model,
        b"\x1b]633;E;cmd;foreign\x07\x1b]633;C\x07\x1b]633;D;0\x07",
        Some("own"),
    );
    assert_eq!(model.state(), ShellCommandState::default());
    feed(&mut model, b"\x1b]633;C\x07\x1b]633;D;0\x07", Some("own"));
    assert_eq!(model.state(), ShellCommandState::default());
}

#[test]
fn shell_command_state_osc133_without_configured_nonce() {
    let mut model = ShellCommandStateModel::default();
    feed(&mut model, b"\x1b]133;C\x1b\\", None);
    assert_eq!(model.state().activity, ShellActivity::Running);
    feed(&mut model, b"\x1b]133;D\x1b\\", None);
    assert_eq!(
        model.state(),
        ShellCommandState {
            activity: ShellActivity::Idle,
            last_exit_code: None
        }
    );
}

#[test]
fn shell_command_state_prompt_does_not_guess_completion() {
    let mut model = ShellCommandStateModel::default();
    feed(&mut model, b"\x1b]133;C\x07\x1b]133;A\x07", None);
    assert_eq!(model.state().activity, ShellActivity::Running);
}
