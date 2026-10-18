use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, CloseRestoreTabState};
use crate::plan::OrchestratorRef;

fn inputs() -> (Plan, AuthenticatedPlanCaller, Tab, Tab) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner_id = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner_id).unwrap();
    let team = mux
        .resolve_or_mint_team_for_parent(&owner_id)
        .unwrap()
        .team_id;
    let utility_id = mux.create_tab(&workspace).unwrap();
    let owner = mux.get_tab_mut(&owner_id).unwrap();
    owner.agent_type = Some("provider".into());
    owner.agent_session_id = Some("owner-session".into());
    owner.session_binding_nonce = Some("owner-nonce".into());
    owner.reported_session_binding_nonce = Some("owner-nonce".into());
    owner.chat_view = true;
    owner.pty_id = Some("owner-pty".into());
    let owner = owner.clone();
    let utility = mux.get_tab_mut(&utility_id).unwrap();
    utility.team_id = Some(team.clone());
    utility.parent_tab_id = Some(owner_id.clone());
    utility.close_restore = Some(CloseRestoreTabState {
        utility_owner_tab_id: Some(owner_id.clone()),
        ..Default::default()
    });
    utility.session_binding_nonce = Some("utility-nonce".into());
    utility.pty_id = Some("utility-pty".into());
    let plan = Plan::new(
        "plan".into(),
        "project".into(),
        OrchestratorRef {
            tab_id: Some(owner_id),
            session_id: Some("owner-session".into()),
            title: None,
        },
    );
    let plan = Plan {
        team_id: Some(team),
        ..plan
    };
    let caller = AuthenticatedPlanCaller {
        tab_id: Some(utility_id),
        session_id: None,
        binding_nonce: Some("utility-nonce".into()),
    };
    (plan, caller, utility.clone(), owner)
}

#[test]
fn owned_utility_selects_live_bound_plan_owner_without_a_shell_agent_session() {
    let (plan, caller, utility, owner) = inputs();
    assert!(owned_utility_can_clean(
        &plan,
        &caller,
        Some(&utility),
        Some(&owner)
    ));
    assert!(!owned_utility_can_clean(&plan, &caller, None, Some(&owner)));
    assert!(!owned_utility_can_clean(
        &plan,
        &caller,
        Some(&utility),
        None
    ));
}

#[test]
fn utility_selection_refuses_foreign_scope_and_unproven_callers() {
    type Change = fn(&mut Plan, &mut AuthenticatedPlanCaller, &mut Tab, &mut Tab);
    let cases: &[(&str, Change)] = &[
        ("missing nonce", |_, c, _, _| c.binding_nonce = None),
        ("foreign nonce", |_, c, _, _| {
            c.binding_nonce = Some("foreign".into())
        }),
        ("foreign tab", |_, c, _, _| {
            c.tab_id = Some("foreign".into())
        }),
        ("claimed owner session", |_, c, _, _| {
            c.session_id = Some("owner-session".into())
        }),
        ("foreign plan", |p, _, _, _| {
            p.owner.tab_id = Some("foreign".into())
        }),
        ("foreign plan Team", |p, _, _, _| {
            p.team_id = Some("foreign".into())
        }),
        ("no plan Team", |p, _, _, _| p.team_id = None),
        ("foreign utility Team", |_, _, u, _| {
            u.team_id = Some("foreign".into())
        }),
        ("foreign owner Team", |_, _, _, o| {
            o.team_id = Some("foreign".into())
        }),
        ("arbitrary shell", |_, _, u, _| u.close_restore = None),
        ("parent alone", |_, _, u, _| {
            u.close_restore.as_mut().unwrap().utility_owner_tab_id = None
        }),
        ("foreign creator", |_, _, u, _| {
            u.close_restore.as_mut().unwrap().utility_owner_tab_id = Some("foreign".into())
        }),
        ("foreign parent", |_, _, u, _| {
            u.parent_tab_id = Some("foreign".into())
        }),
        ("Worker", |_, _, u, _| u.role = Some(AgentRole::Worker)),
        ("role profile", |_, _, u, _| {
            u.role_profile = Some("planner".into())
        }),
        ("agent shell", |_, _, u, _| {
            u.agent_type = Some("provider".into())
        }),
        ("registered shell", |_, _, u, _| {
            u.agent_session_id = Some("session".into())
        }),
        ("stopped shell", |_, _, u, _| u.pty_id = None),
        ("dead shell", |_, _, u, _| {
            u.exit_suspected_at = Some(std::time::Instant::now())
        }),
        ("dead owner", |_, _, _, o| o.pty_id = None),
        ("exited owner", |_, _, _, o| {
            o.exit_suspected_at = Some(std::time::Instant::now())
        }),
        ("unbound owner", |_, _, _, o| o.agent_session_id = None),
        ("stale owner nonce", |_, _, _, o| {
            o.reported_session_binding_nonce = None
        }),
        ("rebound owner", |_, _, _, o| {
            o.agent_session_id = Some("new-session".into())
        }),
        ("unbound plan owner", |p, _, _, _| p.owner.session_id = None),
        ("former Orchestrator", |_, _, _, o| {
            o.role = Some(AgentRole::Worker)
        }),
    ];
    for (name, change) in cases {
        let (mut plan, mut caller, mut utility, mut owner) = inputs();
        change(&mut plan, &mut caller, &mut utility, &mut owner);
        assert!(
            !owned_utility_can_clean(&plan, &caller, Some(&utility), Some(&owner)),
            "{name}"
        );
    }
}

#[test]
fn utility_cleanup_does_not_grant_generic_planning_authority() {
    let (fixture, plan_id, _, owner) = super::super::test_fixture::fixture();
    let caller = {
        let mut mux = fixture.service.mux.write();
        let owner_id = owner.tab_id.as_deref().unwrap();
        let workspace = mux.find_workspace_for_tab(owner_id).unwrap();
        let team = mux.get_tab(owner_id).unwrap().team_id.clone();
        let utility_id = mux.create_tab(&workspace).unwrap();
        let utility = mux.get_tab_mut(&utility_id).unwrap();
        utility.team_id = team;
        utility.parent_tab_id = Some(owner_id.into());
        utility.close_restore = Some(CloseRestoreTabState {
            utility_owner_tab_id: Some(owner_id.into()),
            ..Default::default()
        });
        utility.session_binding_nonce = Some("utility-nonce".into());
        utility.pty_id = Some("utility-pty".into());
        AuthenticatedPlanCaller {
            tab_id: Some(utility_id),
            session_id: None,
            binding_nonce: Some("utility-nonce".into()),
        }
    };
    assert!(fixture
        .service
        .authorize_plan_lane_cleanup(&plan_id, &caller)
        .is_ok());
    assert!(fixture
        .service
        .authorize_plan_orchestrator(&plan_id, &caller)
        .is_err());
    assert!(fixture
        .service
        .authorize_draft_structure_mutation(&plan_id, &caller)
        .is_err());
}
