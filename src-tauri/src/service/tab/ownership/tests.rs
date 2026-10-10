use super::authorize_remote_ownership_release;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use codeterm_types::pty::{RemoteOwnership, RemoteOwnershipTeam};

fn ownership_fixture() -> (MultiplexerState, String, String, String, String, String) {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&orchestrator).unwrap();
    mux.set_tab_role_kind(
        &orchestrator,
        AgentRole::Manager,
        Some("orchestrator"),
        None,
    )
    .unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let target_id = mux.create_tab(&workspace).unwrap();
    let planner = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role_kind(
        &planner,
        AgentRole::Manager,
        Some("planner"),
        Some(&orchestrator),
    )
    .unwrap();
    let unrelated = mux.create_tab(&workspace).unwrap();
    let general_agent = mux.create_tab(&workspace).unwrap();
    {
        let target = mux.get_tab_mut(&target_id).unwrap();
        target.team_id = Some(team_id.clone());
        target.ownership = Some(RemoteOwnership {
            controller_title: "Orchestrator".into(),
            controller_tab_id: None,
            peer_id: "peer-eight".into(),
            peer_name: "eight".into(),
            team: Some(RemoteOwnershipTeam {
                id: team_id,
                title: None,
            }),
        });
        let general = mux.get_tab_mut(&general_agent).unwrap();
        general.role = Some(AgentRole::Manager);
        general.role_profile = Some(crate::service::general_agent::GENERAL_AGENT_KIND.into());
    }
    (
        mux,
        target_id,
        orchestrator,
        planner,
        unrelated,
        general_agent,
    )
}

fn assert_allowed(mux: &MultiplexerState, target_id: &str, caller_tab_id: Option<&str>) {
    let target = mux.get_tab(target_id).unwrap();
    authorize_remote_ownership_release(
        mux,
        target,
        target.ownership.as_ref().unwrap(),
        caller_tab_id,
    )
    .expect("caller is allowed to release remote ownership");
}

fn assert_refused(mux: &MultiplexerState, target_id: &str, caller_tab_id: &str) {
    let target = mux.get_tab(target_id).unwrap();
    assert!(authorize_remote_ownership_release(
        mux,
        target,
        target.ownership.as_ref().unwrap(),
        Some(caller_tab_id),
    )
    .is_err());
}

#[test]
fn local_user_general_agent_target_and_team_orchestrator_can_release() {
    let (mux, target, orchestrator, _, _, general_agent) = ownership_fixture();
    assert_allowed(&mux, &target, None);
    assert_allowed(&mux, &target, Some(&target));
    assert_allowed(&mux, &target, Some(&general_agent));
    assert_allowed(&mux, &target, Some(&orchestrator));
}

#[test]
fn unrelated_tabs_and_team_planners_cannot_release() {
    let (mux, target, _, planner, unrelated, _) = ownership_fixture();
    assert_refused(&mux, &target, &planner);
    assert_refused(&mux, &target, &unrelated);
}
