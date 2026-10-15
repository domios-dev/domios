use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::{AgentRole, Tab};
use codeterm_service_core::plan::{OrchestratorRef, Plan};
use codeterm_service_core::service::{
    execute_transition_at, AgentTransitionRequest, TransitionFailurePoint,
};

fn arm_session(tab: &mut Tab, suffix: &str) {
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some(format!("session-{suffix}"));
    tab.session_binding_nonce = Some(format!("nonce-{suffix}"));
    tab.reported_session_binding_nonce = Some(format!("nonce-{suffix}"));
}

#[test]
fn final_orchestrator_unlink_retains_team_and_plan_for_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let store = codeterm_service_core::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, owner) = MultiplexerState::new();
    arm_session(mux.get_tab_mut(&owner).unwrap(), "owner");
    mux.set_orchestrator_role(&owner).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    let workspace_id = mux.find_workspace_for_tab(&owner).unwrap();
    let planner = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(&planner, AgentRole::Manager, Some("planner"), Some(&owner))
        .unwrap();
    arm_session(mux.get_tab_mut(&planner).unwrap(), "planner");
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&owner))
        .unwrap();
    arm_session(mux.get_tab_mut(&worker).unwrap(), "worker");
    let mut plan = Plan::new(
        "Recovery continuity".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner),
    );
    plan.team_id = Some(team_id.clone());
    plan.planner = Some(codeterm_service_core::plan::PlannerRef::Tab {
        tab_id: planner.clone(),
        session_id: None,
        title: None,
    });
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            codeterm_service_core::service::plan::NewTask {
                name: "Stale descendant".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .set_task_assignment(&task_id, Some(&worker), Some("worker"), 3)
        .unwrap();

    execute_transition_at(
        &mut mux,
        &store,
        AgentTransitionRequest::roleless(owner.clone()),
        TransitionFailurePoint::None,
        4,
    )
    .unwrap();

    assert_eq!(mux.list_teams().len(), 1);
    assert!(mux.get_team(&team_id).is_some());
    let retired = store.get(&plan.id).unwrap();
    assert_eq!(retired.id, plan.id);
    assert_eq!(retired.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(
        retired.status,
        codeterm_service_core::plan::PlanStatus::Active
    );
    assert_eq!(retired.owner.tab_id, None);
    assert!(retired.planner.is_none());
    assert!(retired.tasks[0].assigned_tab_id.is_none());
    let retirement = retired.owner_history.last().unwrap();
    assert_eq!(retirement.from.tab_id.as_deref(), Some(owner.as_str()));
    assert_eq!(retirement.to.tab_id, None);

    for stale_caller in [&planner, &worker] {
        let error = crate::team::prepare_team_member_spawn_with_provider_validation(
            &mux,
            &team_id,
            Some(stale_caller),
            crate::team::AddTeamMemberParams {
                provider: "provider-neutral".into(),
                role: "orchestrator".into(),
                title: None,
                task: None,
                model: None,
            },
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.starts_with(codeterm_types::team::staffing::TEAM_CALLER_UNAUTHORIZED));
    }

    let prepared = crate::team::prepare_team_member_spawn_with_provider_validation(
        &mux,
        &team_id,
        None,
        crate::team::AddTeamMemberParams {
            provider: "provider-neutral".into(),
            role: "orchestrator".into(),
            title: None,
            task: None,
            model: None,
        },
        |_| Ok(()),
    )
    .unwrap();
    let replacement = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&replacement).unwrap();
    arm_session(mux.get_tab_mut(&replacement).unwrap(), "replacement");
    crate::team::commit_team_member_spawn(
        &mut mux,
        &codeterm_events::EventBroadcaster::new(),
        &team_id,
        None,
        &replacement,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .unwrap();
    let takeover =
        crate::team::take_over_team_plan(&mut mux, &store, &team_id, &replacement, 5).unwrap();

    assert_eq!(takeover.error, None);
    assert_eq!(takeover.plan_id, plan.id);
    assert_eq!(mux.list_teams().len(), 1);
    let live_anchors: Vec<_> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| {
            tab.team_id.as_deref() == Some(team_id.as_str())
                && tab.anchors_team()
                && tab.agent_type.is_some()
        })
        .map(|tab| tab.id.clone())
        .collect();
    assert_eq!(live_anchors, vec![replacement.clone()]);
    let recovered = store.get(&plan.id).unwrap();
    assert_eq!(
        recovered.owner.tab_id.as_deref(),
        Some(replacement.as_str())
    );
    assert_eq!(recovered.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(
        recovered.status,
        codeterm_service_core::plan::PlanStatus::Active
    );
    assert_eq!(recovered.owner_history.len(), 2);
}
