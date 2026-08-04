use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;

fn general_mux() -> (MultiplexerState, String) {
    let mut mux = MultiplexerState::empty();
    crate::service::general_agent::ensure_general_workspace(&mut mux, false);
    let tab_id = mux
        .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
        .expect("General alias");
    (mux, tab_id)
}

fn general_tab() -> crate::multiplexer::types::Tab {
    let (mux, tab_id) = general_mux();
    mux.get_tab(&tab_id).expect("General Tab").clone()
}

#[test]
fn general_agent_is_an_anchoring_manager_kind() {
    let tab = general_tab();

    assert_eq!(tab.role, Some(AgentRole::Manager));
    assert_eq!(tab.role_profile.as_deref(), Some(GENERAL_AGENT_KIND));
    assert_eq!(tab.role_kind_id(), Some(GENERAL_AGENT_KIND));
    assert!(tab.anchors_team(), "General Agent is the second anchor");
    assert!(tab.kind_can_spawn());
    assert!(tab.has_team_authority());
    assert_eq!(tab.team_id, None, "an anchor mints its Team at first spawn");
    assert_eq!(tab.parent_tab_id, None);
}

#[test]
fn general_agent_gains_team_authority_only_with_a_live_team() {
    let mut tab = general_tab();

    assert!(!tab.has_team_orchestrator_authority());
    tab.team_id = Some("team_abcd1234".into());
    assert!(
        tab.has_team_orchestrator_authority(),
        "the second anchor holds real Team authority once it owns a Team"
    );
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

#[test]
fn an_orchestrator_never_gains_machine_coordination_capabilities() {
    let mut tab = general_tab();
    tab.role_profile = Some("orchestrator".into());

    assert!(capabilities(&tab).is_none());
    assert!(!is_general_agent(&tab));
}

#[test]
fn the_sanctioned_general_tab_survives_confinement() {
    let (mut mux, tab_id) = general_mux();

    assert!(confine_general_agent_kind(&mut mux).is_empty());
    let tab = mux.get_tab(&tab_id).expect("General Tab");
    assert_eq!(tab.role_kind_id(), Some(GENERAL_AGENT_KIND));
}

#[test]
fn a_general_agent_kind_outside_the_general_workspace_is_stripped() {
    let (mut mux, general_id) = general_mux();
    let other_ws = mux.create_workspace("Project".to_string(), None);
    let stray = mux.create_tab(&other_ws.id).expect("stray Tab");
    if let Some(tab) = mux.get_tab_mut(&stray) {
        apply_general_agent_kind(tab);
        tab.team_id = Some("team_forged1".into());
    }

    let stripped = confine_general_agent_kind(&mut mux);

    assert_eq!(stripped, vec![stray.clone()]);
    let stray_tab = mux.get_tab(&stray).expect("stray Tab remains");
    assert_eq!(stray_tab.role, None);
    assert_eq!(stray_tab.role_profile, None);
    assert_eq!(stray_tab.team_id, None);
    assert!(!stray_tab.can_spawn);
    assert!(!stray_tab.anchors_team());
    assert_eq!(
        mux.get_tab(&general_id).expect("General Tab").role_kind_id(),
        Some(GENERAL_AGENT_KIND),
        "confinement never touches the sanctioned tab"
    );
}

#[test]
fn a_second_general_agent_inside_the_general_workspace_is_stripped() {
    let (mut mux, general_id) = general_mux();
    let general_ws = mux
        .find_workspace_for_tab(&general_id)
        .expect("General workspace");
    let sibling = mux.create_tab(&general_ws).expect("sibling Tab");
    if let Some(tab) = mux.get_tab_mut(&sibling) {
        apply_general_agent_kind(tab);
    }

    let stripped = confine_general_agent_kind(&mut mux);

    assert_eq!(
        stripped,
        vec![sibling.clone()],
        "only the aliased General tab keeps the machine kind"
    );
    assert_eq!(mux.get_tab(&sibling).expect("sibling").role, None);
}
