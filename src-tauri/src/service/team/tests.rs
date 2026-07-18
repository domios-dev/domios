use super::*;

use crate::api::events::SystemEvent;

fn team_members() -> (MultiplexerState, String, String, String, String) {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_pane(&orchestrator).unwrap();
    let planner = mux.create_tab(&workspace_id).unwrap().1;
    mux.set_pane_role(&planner, AgentRole::Manager, Some(&orchestrator))
        .unwrap();
    mux.get_pane_mut(&planner).unwrap().role_profile = Some("planner".into());
    let worker = mux.create_tab(&workspace_id).unwrap().1;
    mux.set_pane_role(&worker, AgentRole::Worker, Some(&orchestrator))
        .unwrap();
    (mux, team_id, orchestrator, planner, worker)
}

#[test]
fn team_manager_permission_allows_orchestrator_and_planner() {
    let (mux, team_id, orchestrator, planner, _) = team_members();
    assert!(authorize_team_manager(&mux, &team_id, None).is_ok());
    assert!(authorize_team_manager(&mux, &team_id, Some(&orchestrator)).is_ok());
    assert!(authorize_team_manager(&mux, &team_id, Some(&planner)).is_ok());
}

#[test]
fn mux_rename_team_rejects_worker_and_foreign_pane() {
    let (mut mux, team_id, _, _, worker) = team_members();
    assert!(authorize_team_manager(&mux, &team_id, Some(&worker)).is_err());
    let foreign = mux.create_pane();
    assert!(authorize_team_manager(&mux, &team_id, Some(&foreign)).is_err());
}

#[test]
fn team_member_role_accepts_only_worker_or_planner() {
    assert_eq!(team_member_role("worker").unwrap(), ("Worker".into(), None));
    assert_eq!(
        team_member_role("planner").unwrap(),
        ("Manager".into(), Some("planner".into()))
    );
    assert!(team_member_role("manager").is_err());
}

#[test]
fn close_team_removes_registry_and_members_with_one_state_change() {
    let (mut mux, team_id, orchestrator, _, _) = team_members();
    let plan_store = crate::service::plan::PlanStore::new();
    let workspace_id = mux.find_workspace_for_pane(&orchestrator).unwrap();
    let worker_a = spawn_fixture_member(
        &mut mux,
        &workspace_id,
        &orchestrator,
        AgentRole::Worker,
        None,
    );
    let worker_b = spawn_fixture_member(
        &mut mux,
        &workspace_id,
        &orchestrator,
        AgentRole::Worker,
        None,
    );
    let events = crate::api::events::EventBroadcaster::new();
    let mut subscription = events.subscribe();

    let (closed, _, _) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        Some(&orchestrator),
        1,
    )
    .unwrap();

    assert_eq!(closed.id, team_id);
    assert!(mux.get_team(&team_id).is_none());
    assert!(mux.get_pane(&orchestrator).is_none());
    assert!(mux.get_pane(&worker_a).is_none());
    assert!(mux.get_pane(&worker_b).is_none());
    assert!(matches!(
        subscription.try_recv().unwrap().event,
        SystemEvent::MuxStateChanged
    ));
    assert!(matches!(
        subscription.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[test]
fn close_team_parks_and_detaches_bound_plan() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, _, _) = team_members();
    let plan = plan_store
        .create_plan_for_team(
            "Bound".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_pane(&orchestrator),
            Some(team_id.clone()),
            vec![],
            1,
        )
        .unwrap();
    let events = crate::api::events::EventBroadcaster::new();
    let (_, _, changed_plan_ids) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        Some(&orchestrator),
        2,
    )
    .unwrap();

    let parked = plan_store.get(&plan.id).unwrap();
    assert!(mux.get_team(&team_id).is_none());
    assert_eq!(changed_plan_ids, vec![plan.id.clone()]);
    assert_eq!(parked.team_id, None);
    assert_eq!(parked.mode, crate::plan::PlanMode::Paused);
    assert!(plan_store
        .list_index()
        .iter()
        .any(|entry| entry.plan_id == plan.id));
}

#[test]
fn close_team_staging_failure_leaves_mux_and_plan_unchanged_without_events() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    let workspace_id = mux.find_workspace_for_pane(&orchestrator).unwrap();
    let independent = mux.create_tab(&workspace_id).unwrap().1;
    mux.get_pane_mut(&independent).unwrap().team_id = Some(team_id.clone());
    let plan = plan_store
        .create_plan_for_team(
            "Bound".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_pane(&orchestrator),
            Some(team_id.clone()),
            vec![],
            1,
        )
        .unwrap();
    let before_plan = plan_store.get(&plan.id).unwrap();
    let before_members = [
        orchestrator.clone(),
        planner.clone(),
        worker.clone(),
        independent,
    ];
    let before_snapshot = snapshot_without_generation(&mux);
    let events = crate::api::events::EventBroadcaster::new();
    let mut subscription = events.subscribe();

    let error = close_team_with_state_change_using(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        Some(&orchestrator),
        2,
        {
            let mut calls = 0;
            move |staged_mux, pane_id| {
                calls += 1;
                if calls == 2 {
                    Err("forced mux close failure".into())
                } else {
                    staged_mux.close_pane_for_team_close(pane_id)
                }
            }
        },
    )
    .unwrap_err();

    assert_eq!(error, "forced mux close failure");
    assert!(mux.get_team(&team_id).is_some());
    assert!(before_members
        .iter()
        .all(|pane_id| mux.get_pane(pane_id).is_some()));
    assert_eq!(snapshot_without_generation(&mux), before_snapshot);
    assert_eq!(plan_store.get(&plan.id).unwrap(), before_plan);
    assert!(matches!(
        subscription.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

fn snapshot_without_generation(mux: &MultiplexerState) -> serde_json::Value {
    let mut snapshot = serde_json::to_value(mux.snapshot()).unwrap();
    snapshot["generation"] = serde_json::Value::Null;
    snapshot
}

#[test]
fn dangling_binding_sweep_parks_orphan_plans_once() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mux, _, orchestrator, _, _) = team_members();
    let plan = plan_store
        .create_plan_for_team(
            "Orphan".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_pane(&orchestrator),
            Some("team_missing".into()),
            vec![],
            1,
        )
        .unwrap();

    assert_eq!(
        reconcile_dangling_team_plan_bindings(&mux, &plan_store, 2).unwrap(),
        vec![plan.id.clone()]
    );
    let repaired = plan_store.get(&plan.id).unwrap();
    assert_eq!(repaired.team_id, None);
    assert_eq!(repaired.mode, crate::plan::PlanMode::Paused);
    assert!(reconcile_dangling_team_plan_bindings(&mux, &plan_store, 3)
        .unwrap()
        .is_empty());
}

#[test]
fn binding_sweep_recovers_populated_legacy_plan_from_empty_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mux, team_id, orchestrator, _, _) = team_members();
    let legacy = plan_store
        .create_plan(
            "Recovered effort".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_pane("retired-owner"),
            vec![],
            1,
        )
        .unwrap();
    for index in 0..40 {
        plan_store
            .add_task(
                &legacy.id,
                crate::service::plan::NewTask {
                    name: format!("Recovered task {index}"),
                    ..Default::default()
                },
                2 + index,
            )
            .unwrap();
    }
    let placeholder = plan_store
        .create_plan_for_team(
            legacy.title.clone(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_pane(&orchestrator),
            Some(team_id.clone()),
            vec![],
            50,
        )
        .unwrap();

    let mut changed = reconcile_dangling_team_plan_bindings(&mux, &plan_store, 100).unwrap();
    changed.sort();
    let mut expected = vec![legacy.id.clone(), placeholder.id.clone()];
    expected.sort();
    assert_eq!(changed, expected);
    let recovered = plan_store.get(&legacy.id).unwrap();
    assert_eq!(recovered.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(recovered.tasks.len(), 40);
    let parked = plan_store.get(&placeholder.id).unwrap();
    assert_eq!(parked.team_id, None);
    assert_eq!(parked.mode, crate::plan::PlanMode::Paused);
    assert!(
        reconcile_dangling_team_plan_bindings(&mux, &plan_store, 101)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn ordinary_close_preserves_team_but_explicit_close_removes_it() {
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    let plan_store = crate::service::plan::PlanStore::new();
    mux.close_pane(&orchestrator).unwrap();
    mux.close_pane(&planner).unwrap();
    mux.close_pane(&worker).unwrap();
    assert!(mux.get_team(&team_id).is_some());
    let events = crate::api::events::EventBroadcaster::new();
    close_team_with_state_change(&mut mux, &events, &plan_store, &team_id, None, 1).unwrap();
    assert!(mux.get_team(&team_id).is_none());
}

#[test]
fn close_team_unknown_and_repeat_are_structured_errors() {
    let (mut mux, team_id, orchestrator, _, _) = team_members();
    let plan_store = crate::service::plan::PlanStore::new();
    let events = crate::api::events::EventBroadcaster::new();
    assert!(close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        "team_missing",
        Some(&orchestrator),
        1,
    )
    .unwrap_err()
    .contains("not found"));
    close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        Some(&orchestrator),
        1,
    )
    .unwrap();
    assert!(close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        Some(&orchestrator),
        1,
    )
    .unwrap_err()
    .contains("not found"));
}

#[test]
fn close_team_removes_chat_backend_member_and_registry() {
    let (mut mux, team_id, orchestrator, _, worker) = team_members();
    let plan_store = crate::service::plan::PlanStore::new();
    mux.get_pane_mut(&worker).unwrap().backing = PaneBacking::ChatBackend {
        plugin_id: "chat-backend".into(),
        session_id: "session-1".into(),
    };
    let events = crate::api::events::EventBroadcaster::new();

    let (_, cleanup, _) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        Some(&orchestrator),
        1,
    )
    .unwrap();

    assert!(mux.get_team(&team_id).is_none());
    assert!(mux.get_pane(&worker).is_none());
    assert_eq!(
        cleanup.chat_backings,
        vec![("chat-backend".into(), "session-1".into())]
    );
}

fn spawn_fixture_member(
    mux: &mut MultiplexerState,
    workspace_id: &str,
    parent: &str,
    role: AgentRole,
    profile: Option<&str>,
) -> String {
    let pane_id = mux.create_tab(workspace_id).unwrap().1;
    mux.set_pane_role(&pane_id, role, Some(parent)).unwrap();
    mux.get_pane_mut(&pane_id).unwrap().role_profile = profile.map(str::to_string);
    pane_id
}

#[test]
fn add_team_member_uses_existing_manager_and_team() {
    let (mut mux, team_id, manager, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_pane(&manager).unwrap();
    let before_count = mux.team_count();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        &manager,
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "worker".into(),
            title: None,
            task: None,
            model: None,
        },
    )
    .unwrap();
    let pane_id = spawn_fixture_member(&mut mux, &workspace_id, &manager, AgentRole::Worker, None);
    let events = crate::api::events::EventBroadcaster::new();
    let result = commit_team_member_spawn(
        &mut mux,
        &events,
        &team_id,
        &manager,
        &pane_id,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .unwrap();
    let pane = mux.get_pane(&result.pane_id).unwrap();
    assert_eq!(pane.parent_pane_id.as_deref(), Some(manager.as_str()));
    assert_eq!(pane.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(pane.role, Some(AgentRole::Worker));
    assert_eq!(
        mux.find_workspace_for_pane(&result.pane_id).as_deref(),
        Some(workspace_id.as_str())
    );
    assert_eq!(mux.team_count(), before_count);
    assert_eq!(result.team.id, team_id);
}

#[test]
fn add_team_member_sets_exact_planner_role_and_emits_one_state_change() {
    let (mut mux, team_id, manager, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_pane(&manager).unwrap();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        &manager,
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "planner".into(),
            title: None,
            task: None,
            model: None,
        },
    )
    .unwrap();
    let pane_id = spawn_fixture_member(
        &mut mux,
        &workspace_id,
        &manager,
        AgentRole::Manager,
        Some("planner"),
    );
    let events = crate::api::events::EventBroadcaster::new();
    let mut subscription = events.subscribe();
    let result = commit_team_member_spawn(
        &mut mux,
        &events,
        &team_id,
        &manager,
        &pane_id,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .unwrap();
    let pane = mux.get_pane(&result.pane_id).unwrap();
    assert_eq!(pane.role, Some(AgentRole::Manager));
    assert_eq!(pane.role_profile.as_deref(), Some("planner"));
    assert!(matches!(
        subscription.try_recv().unwrap().event,
        SystemEvent::MuxStateChanged
    ));
    assert!(matches!(
        subscription.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[test]
fn team_member_role_accepts_orchestrator() {
    assert_eq!(
        team_member_role("orchestrator").unwrap(),
        ("Orchestrator".into(), None)
    );
}

#[test]
fn add_team_member_rejects_worker_foreign_and_unspawnable_without_mutation() {
    let (mut mux, team_id, manager, _, worker) = team_members();
    let workspace_id = mux.find_workspace_for_pane(&manager).unwrap();
    let (worker, foreign) = {
        let foreign = mux.create_tab(&workspace_id).unwrap().1;
        (worker, foreign)
    };
    let before_panes = mux.pane_count_in_workspace(&workspace_id);
    let before_teams = mux.team_count();
    for caller in [&worker, &foreign] {
        assert!(prepare_team_member_spawn_with_provider_validation(
            &mux,
            &team_id,
            caller,
            AddTeamMemberParams {
                provider: "unknown-provider".into(),
                role: "worker".into(),
                title: None,
                task: None,
                model: None
            },
            |_| Ok(()),
        )
        .is_err());
    }
    for provider in ["unknown-provider", "incapable-provider"] {
        let error = prepare_team_member_spawn_with_provider_validation(
            &mux,
            &team_id,
            &manager,
            AddTeamMemberParams {
                provider: provider.into(),
                role: "worker".into(),
                title: None,
                task: None,
                model: None,
            },
            |spawn| {
                Err(format!(
                    "provider {} is not spawn-capable",
                    spawn.agent.as_deref().unwrap_or_default()
                ))
            },
        )
        .unwrap_err();
        assert!(error.contains(provider));
    }
    let expected = prepare_team_member_spawn_with_provider_validation(
        &mux,
        &team_id,
        &manager,
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "worker".into(),
            title: None,
            task: None,
            model: None,
        },
        |_| Ok(()),
    )
    .unwrap();
    let pane_id = spawn_fixture_member(&mut mux, &workspace_id, &manager, AgentRole::Worker, None);
    mux.get_pane_mut(&pane_id).unwrap().team_id = None;
    let events = crate::api::events::EventBroadcaster::new();
    let mut subscription = events.subscribe();
    assert!(commit_team_member_spawn(
        &mut mux,
        &events,
        &team_id,
        &manager,
        &pane_id,
        &expected.expected_role,
        &expected.expected_profile
    )
    .is_err());
    assert!(mux.get_pane(&pane_id).is_none());
    assert!(matches!(
        subscription.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(mux.pane_count_in_workspace(&workspace_id), before_panes);
    assert_eq!(mux.team_count(), before_teams);
}
