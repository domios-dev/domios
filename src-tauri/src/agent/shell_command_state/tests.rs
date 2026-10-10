use super::*;
use crate::agent::semantic_marker::SemanticMarkerParser;

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
    let mut tracker = crate::agent::command_detection::ShellCapabilityTracker::new();
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

#[test]
fn shell_command_state_isolated_from_agent_status_and_foreign_pty() {
    use crate::multiplexer::state::MultiplexerState;
    let mut mux = MultiplexerState::empty();
    let workspace = mux.create_workspace("Shell".into(), None);
    let tab_id = mux.create_tab(&workspace.id).unwrap();
    mux.link_pty(&tab_id, "own".into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    assert!(tab.is_utility_shell());
    let state = ShellCommandState {
        activity: ShellActivity::Running,
        last_exit_code: None,
    };
    assert!(!apply_shell_command_state(tab, "foreign", state));
    assert_eq!(tab.shell_command_state, ShellCommandState::default());
    assert!(apply_shell_command_state(tab, "own", state));
    assert!(tab.agent_state.is_none());
    assert!(tab.pending_delivery.is_none());
    assert!(tab.boot_stage.is_none());
    tab.agent_type = Some("opaque-provider".into());
    let before = serde_json::to_value(&*tab).unwrap();
    assert!(!apply_shell_command_state(
        tab,
        "own",
        ShellCommandState::default()
    ));
    assert_eq!(serde_json::to_value(&*tab).unwrap(), before);
    tab.agent_type = None;
    tab.roleless_tracked = true;
    assert!(!apply_shell_command_state(
        tab,
        "own",
        ShellCommandState::default()
    ));
}

#[test]
fn shell_command_state_pending_utility_command_ignores_initial_prompt() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&id, "own".into());
    let tab = mux.get_tab_mut(&id).unwrap();
    // The bootstrap tab is the Orchestrator; strip its role so it is a plain utility shell.
    tab.role = None;
    tab.role_profile = None;
    tab.utility_command_pending = true;
    let idle = ShellCommandState {
        activity: ShellActivity::Idle,
        last_exit_code: None,
    };
    assert!(!apply_shell_command_state(tab, "own", idle));
    assert!(tab.utility_command_pending);
    assert!(apply_shell_command_state(
        tab,
        "own",
        ShellCommandState {
            activity: ShellActivity::Running,
            last_exit_code: None
        }
    ));
    assert!(!tab.utility_command_pending);
    assert!(apply_shell_command_state(tab, "own", idle));
    tab.utility_command_pending = true;
    assert!(apply_shell_command_state(
        tab,
        "own",
        ShellCommandState {
            activity: ShellActivity::Idle,
            last_exit_code: Some(0)
        }
    ));
    assert!(!tab.utility_command_pending);
}
