//! Both Team membership doors in `service::team`, refused for the General
//! Agent kind and left open for everyone else.

use super::*;

use codeterm_types::team::general_agent_membership::GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION;

use crate::service::general_agent::identity::apply_general_agent_kind;

fn member_params(role: &str) -> AddTeamMemberParams {
    AddTeamMemberParams {
        provider: "spawn-capable".into(),
        role: role.into(),
        title: None,
        task: None,
        model: None,
    }
}

/// An Orchestrator-anchored Team plus a General-Agent-kind tab that has been
/// pushed into it, which is the state the guard has to refuse to build on.
fn team_with_intruding_general_agent() -> (MultiplexerState, String, String, String) {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let general = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&general).unwrap();
    apply_general_agent_kind(tab);
    tab.team_id = Some(team_id.clone());
    (mux, team_id, orchestrator, general)
}

fn team_member_tab_ids(mux: &MultiplexerState, team_id: &str) -> Vec<String> {
    let mut ids: Vec<String> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id))
        .map(|tab| tab.id.clone())
        .collect();
    ids.sort();
    ids
}

#[test]
fn general_agent_team_guard_refuses_add_team_member_by_the_constant_violation_id() {
    let (mux, team_id, _, general) = team_with_intruding_general_agent();
    let before = team_member_tab_ids(&mux, &team_id);
    let before_team_count = mux.team_count();

    let error = prepare_team_member_spawn(&mux, &team_id, Some(&general), member_params("worker"))
        .expect_err("the General Agent may not grow a Team it is inside");

    assert!(
        error.contains(GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION),
        "refusal must carry the stable violation id, got: {error}"
    );
    assert_eq!(
        team_member_tab_ids(&mux, &team_id),
        before,
        "no membership may change on a refused add_team_member"
    );
    assert_eq!(mux.team_count(), before_team_count);
}

#[test]
fn general_agent_team_guard_refuses_add_team_member_before_authorization_masks_it() {
    let (mut mux, _, orchestrator, general) = team_with_intruding_general_agent();
    let foreign_anchor = {
        let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
        mux.create_tab(&workspace_id).unwrap()
    };
    let foreign_team = mux
        .resolve_or_mint_team_for_parent(&foreign_anchor)
        .unwrap()
        .team_id;

    let error = prepare_team_member_spawn(&mux, &foreign_team, Some(&general), member_params("worker"))
        .expect_err("a General Agent outside the Team is refused for identity, not membership");

    assert!(
        error.contains(GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION),
        "the identity refusal must win over the generic not-a-member error, got: {error}"
    );
}

#[test]
fn general_agent_team_guard_refuses_the_anchoring_commit_without_binding_a_team() {
    let (mut mux, team_id, orchestrator, general) = team_with_intruding_general_agent();
    let candidate = {
        let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
        let candidate = mux.create_tab(&workspace_id).unwrap();
        let tab = mux.get_tab_mut(&candidate).unwrap();
        apply_general_agent_kind(tab);
        tab.team_id = None;
        candidate
    };
    let events = crate::api::events::EventBroadcaster::new();

    let error = commit_team_member_spawn(
        &mut mux,
        &events,
        &team_id,
        Some(&general),
        &candidate,
        "Manager",
        &Some(crate::service::general_agent::GENERAL_AGENT_KIND.to_string()),
    )
    .expect_err("an anchoring commit may not bind a Team onto the General Agent kind");

    assert!(
        error.contains(GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION),
        "refusal must carry the stable violation id, got: {error}"
    );
    assert_eq!(
        mux.get_tab(&candidate).unwrap().team_id,
        None,
        "the refused commit must leave the tab unbound"
    );
}

#[test]
fn general_agent_team_guard_leaves_an_ordinary_manager_add_member_byte_identical() {
    let (mut mux, team_id, orchestrator, _) = team_with_intruding_general_agent();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let before_team_count = mux.team_count();

    let prepared =
        prepare_team_member_spawn(&mux, &team_id, Some(&orchestrator), member_params("worker")).unwrap();
    let member = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&member, AgentRole::Worker, Some(&orchestrator))
        .unwrap();
    let events = crate::api::events::EventBroadcaster::new();
    let result = commit_team_member_spawn(
        &mut mux,
        &events,
        &team_id,
        Some(&orchestrator),
        &member,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .expect("an Orchestrator adding a Worker is untouched by the guard");

    assert_eq!(result.team.id, team_id);
    assert_eq!(
        mux.get_tab(&member).unwrap().team_id.as_deref(),
        Some(team_id.as_str())
    );
    assert_eq!(mux.team_count(), before_team_count);
}
