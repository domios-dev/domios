use super::*;

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
