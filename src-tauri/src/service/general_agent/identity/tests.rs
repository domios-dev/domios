use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, MachineAgentIdentity};

fn general_tab() -> crate::multiplexer::types::Tab {
    let mut mux = MultiplexerState::empty();
    crate::service::general_agent::ensure_general_workspace(&mut mux, false);
    let tab_id = mux
        .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
        .expect("General alias");
    mux.get_tab(&tab_id).expect("General Tab").clone()
}

#[test]
fn general_agent_identity_is_machine_scoped_not_team_role() {
    let tab = general_tab();

    assert_eq!(
        tab.machine_agent_identity,
        Some(MachineAgentIdentity::GeneralAgent)
    );
    assert_eq!(tab.role, None);
    assert_eq!(tab.role_profile, None);
    assert_eq!(tab.team_id, None);
    assert_eq!(tab.parent_tab_id, None);
}

#[test]
fn general_agent_cannot_satisfy_team_orchestrator_authority() {
    let mut tab = general_tab();

    assert_ne!(tab.role, Some(AgentRole::Orchestrator));
    assert!(tab.team_id.is_none());
    tab.role = Some(AgentRole::Orchestrator);
    tab.team_id = Some("forged-team".into());
    assert!(!tab.has_team_orchestrator_authority());
}

#[test]
fn general_agent_retains_machine_coordination_capabilities() {
    let tab = general_tab();
    let capabilities = capabilities(&tab).expect("machine General capabilities");

    assert!(tab.can_spawn);
    assert!(capabilities.workspaces);
    assert!(capabilities.teams);
    assert!(capabilities.agent_control);
    assert!(capabilities.mesh);
    assert!(capabilities.bridges);
    assert!(capabilities.cron);
    assert!(capabilities.memory);
}
