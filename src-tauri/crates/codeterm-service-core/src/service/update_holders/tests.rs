use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::TabVerdict;

fn utility_shell() -> (MultiplexerState, String) {
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let shell = mux.create_tab(&workspace).unwrap();
    assert!(mux.get_tab(&shell).unwrap().is_utility_shell());
    (mux, shell)
}

#[test]
fn running_shell_without_a_command_from_its_current_pty_refuses_update() {
    let (mut mux, shell) = utility_shell();
    mux.link_pty(&shell, "old-pty".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    svc.utility_command_started(&shell, "old-pty", Some("old command"));
    {
        let mut mux = svc.mux.write();
        mux.link_pty(&shell, "new-pty".into());
        mux.get_tab_mut(&shell)
            .unwrap()
            .shell_command_state
            .activity = ShellActivity::Running;
    }
    assert_eq!(
        svc.running_utility_command(&shell).as_deref(),
        Some("old command")
    );

    let refusal = svc
        .reserve_update_shutdown(codeterm_update::holders::ReserveHolders {
            tabs: vec![],
            initiating_tab: None,
        })
        .unwrap_err();
    assert!(refusal.contains("The running shell command could not be saved for rerun"));
    assert!(!refusal.contains("old command"));
    svc.capture_shutdown_shells();
    assert!(svc
        .mux
        .read()
        .get_tab(&shell)
        .unwrap()
        .close_restore
        .is_none());
}

#[test]
fn idle_only_policy_still_requires_verified_shell_activity() {
    let (mut mux, shell) = utility_shell();
    mux.link_pty(&shell, "current-pty".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    assert!(svc.require_update_ready(None, false).is_ok());
    assert!(svc
        .require_update_ready(None, true)
        .unwrap_err()
        .contains("Shell activity could not be verified"));
    svc.mux
        .write()
        .get_tab_mut(&shell)
        .unwrap()
        .shell_command_state
        .activity = ShellActivity::Idle;
    assert!(svc.require_update_ready(None, true).is_ok());
}

#[test]
fn idle_only_policy_still_requires_agents_to_be_idle() {
    let (mut mux, agent) = MultiplexerState::new();
    mux.link_pty(&agent, "agent-pty".into());
    mux.get_tab_mut(&agent).unwrap().agent_type = Some("test-agent".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;

    for state in [
        None,
        Some(TimelineState::Working),
        Some(TimelineState::Question),
        Some(TimelineState::Idle),
    ] {
        svc.mux.write().get_tab_mut(&agent).unwrap().agent_state = state.map(|state| {
            TabVerdict {
                state,
                subtype: None,
            }
            .into()
        });
        assert_eq!(
            svc.require_update_ready(None, true).is_ok(),
            state == Some(TimelineState::Idle)
        );
        assert!(svc.require_update_ready(None, false).is_ok());
    }
}
