use super::*;

/// Orchestrator-anchored Team plus its workspace, mirroring the live shape a
/// `team pair` leaves behind.
fn anchored_team() -> (MultiplexerState, String, String, String) {
    let (mut mux, orchestrator) = MultiplexerState::new();
    mux.set_orchestrator_role(&orchestrator).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    (mux, team_id, orchestrator, workspace_id)
}

#[test]
fn a_member_caller_resolves_the_team_anchor() {
    let (mux, team_id, orchestrator, workspace_id) = anchored_team();
    let attachment =
        authorize_team_attachment(&mux, &team_id, &orchestrator, &workspace_id).unwrap();
    assert_eq!(
        attachment,
        TeamAttachment {
            team_id: team_id.clone(),
            caller_tab_id: orchestrator.clone(),
            manager_tab_id: orchestrator,
        }
    );
}

#[test]
fn a_non_member_caller_is_refused_before_any_attach() {
    let (mut mux, team_id, _orchestrator, workspace_id) = anchored_team();
    let outsider = mux.create_tab(&workspace_id).unwrap();
    let error = authorize_team_attachment(&mux, &team_id, &outsider, &workspace_id).unwrap_err();
    assert!(error.contains("does not belong to Team"), "{error}");

    let missing =
        authorize_team_attachment(&mux, &team_id, "tab_ghost", &workspace_id).unwrap_err();
    assert!(missing.contains("not found"), "{missing}");
}

#[test]
fn an_unknown_team_is_refused_even_for_a_live_caller() {
    let (mux, _team_id, orchestrator, workspace_id) = anchored_team();
    let error =
        authorize_team_attachment(&mux, "team_missing", &orchestrator, &workspace_id).unwrap_err();
    assert!(error.contains("team_missing"), "{error}");
}

#[test]
fn spawn_attachment_requires_both_halves_and_stays_absent_without_a_team() {
    let (mux, team_id, orchestrator, workspace_id) = anchored_team();
    assert!(
        resolve_spawn_attachment(&mux, None, Some(&orchestrator), &workspace_id)
            .unwrap()
            .is_none()
    );
    let error = resolve_spawn_attachment(&mux, Some(&team_id), None, &workspace_id).unwrap_err();
    assert!(error.contains("required together"), "{error}");
    let attachment =
        resolve_spawn_attachment(&mux, Some(&team_id), Some(&orchestrator), &workspace_id)
            .unwrap()
            .expect("member caller attaches");
    assert_eq!(attachment.team_id, team_id);
}

#[test]
fn applying_an_attachment_binds_team_parent_and_spawner() {
    let (mut mux, team_id, orchestrator, workspace_id) = anchored_team();
    let attached = mux.create_tab(&workspace_id).unwrap();
    let attachment =
        authorize_team_attachment(&mux, &team_id, &orchestrator, &workspace_id).unwrap();
    apply_team_attachment(&mut mux, &attachment, &attached).unwrap();
    let tab = mux.get_tab(&attached).unwrap();
    assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(tab.parent_tab_id.as_deref(), Some(orchestrator.as_str()));
    assert_eq!(
        tab.spawned_by_tab_id.as_deref(),
        Some(orchestrator.as_str())
    );
    assert_eq!(tab.depth, 2);
}

#[test]
fn adopt_agent_attaches_a_running_roleless_tab() {
    let (mut mux, team_id, orchestrator, workspace_id) = anchored_team();
    let agent_tab = mux.create_tab(&workspace_id).unwrap();
    let team = adopt_agent_into_team(&mut mux, &team_id, &agent_tab, &orchestrator).unwrap();
    assert_eq!(team.id, team_id);
    let tab = mux.get_tab(&agent_tab).unwrap();
    assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(tab.parent_tab_id.as_deref(), Some(orchestrator.as_str()));
}

#[test]
fn adopt_agent_refuses_a_non_member_caller() {
    let (mut mux, team_id, _orchestrator, workspace_id) = anchored_team();
    let outsider = mux.create_tab(&workspace_id).unwrap();
    let agent_tab = mux.create_tab(&workspace_id).unwrap();
    let error = adopt_agent_into_team(&mut mux, &team_id, &agent_tab, &outsider).unwrap_err();
    assert!(error.contains("does not belong to Team"), "{error}");
    assert!(mux.get_tab(&agent_tab).unwrap().team_id.is_none());
}

#[test]
fn adopt_agent_refuses_a_missing_tab_and_an_already_bound_tab() {
    let (mut mux, team_id, orchestrator, workspace_id) = anchored_team();
    let missing =
        adopt_agent_into_team(&mut mux, &team_id, "tab_ghost", &orchestrator).unwrap_err();
    assert!(missing.contains("tab_ghost"), "{missing}");
    assert!(missing.contains("not found"), "{missing}");

    let agent_tab = mux.create_tab(&workspace_id).unwrap();
    adopt_agent_into_team(&mut mux, &team_id, &agent_tab, &orchestrator).unwrap();
    let bound = adopt_agent_into_team(&mut mux, &team_id, &agent_tab, &orchestrator).unwrap_err();
    assert!(bound.contains("already belongs to Team"), "{bound}");
}

#[test]
fn adopt_agent_refuses_a_role_bearing_tab() {
    let (mut mux, team_id, orchestrator, workspace_id) = anchored_team();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_detached_worker_role(&worker, None).unwrap();
    let error = adopt_agent_into_team(&mut mux, &team_id, &worker, &orchestrator).unwrap_err();
    assert!(error.contains("roleless"), "{error}");
}
