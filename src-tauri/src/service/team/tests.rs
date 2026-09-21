use super::*;

use super::staffing::{prepare_team_member_spawn, team_member_role};

use crate::api::events::SystemEvent;
use crate::service::EventBroadcaster;

fn isolated_plan_store() -> (tempfile::TempDir, crate::service::plan::PlanStore) {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    assert!(dir.path().join(".codeterm").join("plans.db").is_file());
    (dir, plan_store)
}

fn team_members() -> (MultiplexerState, String, String, String, String) {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let planner = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &planner,
        AgentRole::Manager,
        Some("planner"),
        Some(&orchestrator),
    )
    .unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
        .unwrap();
    (mux, team_id, orchestrator, planner, worker)
}

#[test]
fn team_manager_permission_allows_orchestrator_and_planner() {
    let (mut mux, team_id, orchestrator, planner, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let custom_manager = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &custom_manager,
        AgentRole::Manager,
        Some("review_lead"),
        Some(&orchestrator),
    )
    .unwrap();
    let by_tab = |tab_id: &str| TeamMutationCaller::AgentTab(tab_id.to_string());
    assert!(authorize_team_manager(&mux, &team_id, &TeamMutationCaller::Human).is_ok());
    assert!(authorize_team_manager(&mux, &team_id, &by_tab(&orchestrator)).is_ok());
    assert!(authorize_team_manager(&mux, &team_id, &by_tab(&planner)).is_ok());
    assert!(
        authorize_team_manager(&mux, &team_id, &by_tab(&custom_manager)).is_err(),
        "an overlay manager kind resolves to leaf capability and holds no Team authority"
    );
    assert!(authorize_team_mutation(&mux, &team_id, &by_tab(&planner)).is_ok());
}

#[test]
fn mux_rename_team_rejects_worker_and_foreign_tab() {
    let (mut mux, team_id, _, _, worker) = team_members();
    assert!(authorize_team_manager(
        &mux,
        &team_id,
        &TeamMutationCaller::AgentTab(worker.clone())
    )
    .is_err());
    let workspace_id = mux.find_workspace_for_tab(&worker).unwrap();
    let foreign = mux.create_tab(&workspace_id).unwrap();
    assert!(
        authorize_team_manager(&mux, &team_id, &TeamMutationCaller::AgentTab(foreign)).is_err()
    );
}

#[test]
fn a_human_plan_transfer_keeps_the_source_refusal_instead_of_borrowing_destination_authority() {
    let (mut mux, _source_team, source_owner, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&source_owner).unwrap();
    let destination_owner = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&destination_owner).unwrap();
    let destination_team = mux
        .resolve_or_mint_team_for_parent(&destination_owner)
        .unwrap()
        .team_id;
    let error = authorize_plan_transfer(
        &mux,
        "team_missing",
        &destination_team,
        &destination_owner,
        TeamMutationCaller::Human,
    )
    .unwrap_err();
    assert!(error.contains("team_missing"));
    assert!(error.contains("not found"));
    assert!(!error.contains("plan rebind authority"));
}

#[test]
fn team_member_role_accepts_builtin_team_roles() {
    assert_eq!(team_member_role("worker").unwrap(), ("Worker".into(), None));
    assert_eq!(
        team_member_role("planner").unwrap(),
        ("Manager".into(), Some("planner".into()))
    );
    assert_eq!(
        team_member_role("orchestrator").unwrap(),
        ("Manager".into(), Some("orchestrator".into()))
    );
    assert_eq!(
        team_member_role("Cartographer").unwrap(),
        ("Manager".into(), Some("cartographer".into()))
    );
    assert!(team_member_role("manager").is_err());
}

#[test]
fn multiple_orchestrators_keep_team_membership_and_spawner_routing_separate() {
    let (mut mux, team_id, first_orchestrator, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&first_orchestrator).unwrap();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        Some(&first_orchestrator),
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "orchestrator".into(),
            title: None,
            task: None,
            model: None,
        },
    )
    .unwrap();
    assert!(prepared.spawn_params.parent_tab_id.is_none());
    assert_eq!(
        prepared.spawn_params.report_target_tab_id.as_deref(),
        Some(first_orchestrator.as_str())
    );
    assert_eq!(
        prepared.spawn_params.workspace_id.as_deref(),
        Some(workspace_id.as_str())
    );

    let second_orchestrator = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&second_orchestrator).unwrap();
    mux.get_tab_mut(&second_orchestrator)
        .unwrap()
        .spawned_by_tab_id = Some(first_orchestrator.clone());
    let events = crate::api::events::EventBroadcaster::new();
    commit_team_member_spawn(
        &mut mux,
        &events,
        &team_id,
        Some(&first_orchestrator),
        &second_orchestrator,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .unwrap();
    assert_eq!(
        mux.get_tab(&second_orchestrator)
            .unwrap()
            .team_id
            .as_deref(),
        Some(team_id.as_str())
    );

    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&second_orchestrator))
        .unwrap();
    let worker = mux.get_tab(&worker).unwrap();
    assert_eq!(worker.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(
        worker.parent_tab_id.as_deref(),
        Some(second_orchestrator.as_str())
    );
}

#[test]
fn plan_owner_uses_the_acting_orchestrator_when_team_has_multiple() {
    let (mut mux, team_id, first_orchestrator, planner, _) = team_members();
    let (plan_dir, plan_store) = isolated_plan_store();
    let workspace_id = mux.find_workspace_for_tab(&first_orchestrator).unwrap();
    let second_orchestrator = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&second_orchestrator).unwrap();
    mux.get_tab_mut(&second_orchestrator).unwrap().team_id = Some(team_id.clone());
    let plan = plan_store
        .create_plan_for_team(
            "Owner resolution".into(),
            plan_dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&first_orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    let by_tab = |tab_id: &str| TeamMutationCaller::AgentTab(tab_id.to_string());

    assert_eq!(
        team_plan_owner(
            &mux,
            &plan_store,
            &team_id,
            &plan.id,
            &by_tab(&second_orchestrator)
        )
        .unwrap(),
        second_orchestrator
    );
    assert_eq!(
        team_plan_owner(&mux, &plan_store, &team_id, &plan.id, &by_tab(&planner)).unwrap(),
        first_orchestrator,
        "a managing leaf resolves through its own chain, not a global scan"
    );
    assert_eq!(
        team_plan_owner(
            &mux,
            &plan_store,
            &team_id,
            &plan.id,
            &TeamMutationCaller::Human
        )
        .unwrap(),
        first_orchestrator,
        "a human names no chain, so the plan's recorded anchor stays the owner"
    );
    assert!(
        team_plan_owner(
            &mux,
            &plan_store,
            &team_id,
            "pl_missing",
            &TeamMutationCaller::Human
        )
        .is_err(),
        "with several anchors and no recorded owner the caller must name one"
    );
}

#[test]
fn close_team_removes_registry_and_members_with_one_state_change() {
    let (mut mux, team_id, orchestrator, _, _) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
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
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap();

    assert_eq!(closed.id, team_id);
    assert!(mux.get_team(&team_id).is_none());
    assert!(mux.get_tab(&orchestrator).is_none());
    assert!(mux.get_tab(&worker_a).is_none());
    assert!(mux.get_tab(&worker_b).is_none());
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
fn close_team_archives_and_detaches_bound_plan() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, planner, _) = team_members();
    let plan = plan_store
        .create_plan_for_team(
            "Bound".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    plan_store
        .set_planner_ref(
            &plan.id,
            Some(crate::plan::PlannerRef::Tab {
                tab_id: planner,
                session_id: None,
                title: None,
            }),
            2,
        )
        .unwrap();
    let events = crate::api::events::EventBroadcaster::new();
    let (_, _, changed_plan_ids) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        3,
    )
    .unwrap();

    let archived = plan_store.get(&plan.id).unwrap();
    assert!(mux.get_team(&team_id).is_none());
    assert_eq!(changed_plan_ids, vec![plan.id.clone()]);
    assert_eq!(archived.team_id, None);
    assert_eq!(archived.status, crate::plan::PlanStatus::Archived);
    assert_eq!(archived.owner, crate::plan::OrchestratorRef::default());
    assert_eq!(archived.planner, None);
    let retired = archived.owner_history.last().unwrap();
    assert_eq!(retired.from.tab_id.as_deref(), Some(orchestrator.as_str()));
    assert_eq!(retired.to, crate::plan::OrchestratorRef::default());
    assert!(plan_store.list_index().iter().any(|entry| {
        entry.plan_id == plan.id && entry.status == crate::plan::PlanStatus::Archived
    }));
}

#[test]
fn close_team_staging_failure_leaves_mux_and_plan_unchanged_without_events() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let independent = mux.create_tab(&workspace_id).unwrap();
    mux.get_tab_mut(&independent).unwrap().team_id = Some(team_id.clone());
    let plan = plan_store
        .create_plan_for_team(
            "Bound".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
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
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        2,
        {
            let mut calls = 0;
            move |staged_mux, tab_id| {
                calls += 1;
                if calls == 2 {
                    Err("forced mux close failure".into())
                } else {
                    staged_mux.close_tab_for_team_close(tab_id)
                }
            }
        },
    )
    .unwrap_err();

    assert_eq!(error, "forced mux close failure");
    assert!(mux.get_team(&team_id).is_some());
    assert!(before_members
        .iter()
        .all(|tab_id| mux.get_tab(tab_id).is_some()));
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
fn dangling_binding_sweep_archives_orphan_plans_once() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mux, _, orchestrator, _, _) = team_members();
    let plan = plan_store
        .create_plan_for_team(
            "Orphan".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&orchestrator),
            "team_missing".into(),
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
    assert_eq!(repaired.status, crate::plan::PlanStatus::Archived);
    assert!(reconcile_dangling_team_plan_bindings(&mux, &plan_store, 3)
        .unwrap()
        .is_empty());
}

#[test]
fn binding_sweep_archives_active_teamless_plan_without_fabricating_a_team() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mux, _, _, _, _) = team_members();
    let mut legacy = crate::plan::Plan::new(
        "Retired effort".into(),
        dir.path().display().to_string(),
        crate::plan::OrchestratorRef::from_tab("retired-owner"),
    );
    plan_store.save(&mut legacy, 1).unwrap();
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
    assert_eq!(
        reconcile_dangling_team_plan_bindings(&mux, &plan_store, 100).unwrap(),
        vec![legacy.id.clone()]
    );
    let archived = plan_store.get(&legacy.id).unwrap();
    assert_eq!(archived.team_id, None);
    assert_eq!(archived.status, crate::plan::PlanStatus::Archived);
    assert_eq!(archived.tasks.len(), 40);
    assert!(
        reconcile_dangling_team_plan_bindings(&mux, &plan_store, 101)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn team_adopt_plan_is_bounded_atomic_and_visible_as_active() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(crate::service::plan::PlanStore::new_for_test(dir.path()));
    let (mut mux, team_id, orchestrator, planner, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let retired_owner = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&retired_owner).unwrap();
    let inherited_worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&inherited_worker, AgentRole::Worker, Some(&retired_owner))
        .unwrap();
    let mut plan = crate::plan::Plan::new(
        "Imported effort".into(),
        dir.path().display().to_string(),
        crate::plan::OrchestratorRef::from_tab(&retired_owner),
    );
    plan.planner = Some(crate::plan::PlannerRef::Tab {
        tab_id: planner,
        session_id: None,
        title: None,
    });
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            crate::service::plan::NewTask {
                name: "Inherited lane".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .set_task_assignment(&task_id, Some(&inherited_worker), Some("worker"), 3)
        .unwrap();
    store
        .update_plan(
            &plan.id,
            None,
            Some(crate::plan::PlanStatus::Archived),
            None,
            4,
        )
        .unwrap();
    let plan_id = plan.id.clone();
    let expected_team_id = team_id.clone();
    let expected_owner = orchestrator.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = adopt_plan_for_team_core(
            &mut mux,
            &store,
            &team_id,
            &plan_id,
            TeamMutationCaller::AgentTab(orchestrator),
            5,
        );
        let inherited_parent = mux
            .get_tab(&inherited_worker)
            .and_then(|tab| tab.parent_tab_id.clone());
        sender.send((result, inherited_parent)).unwrap();
    });

    let (adopted, inherited_parent) = receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("team adopt-plan must not hang");
    let adopted = adopted.unwrap();
    assert_eq!(adopted.team_id.as_deref(), Some(expected_team_id.as_str()));
    assert_eq!(adopted.status, crate::plan::PlanStatus::Active);
    assert_eq!(
        adopted.owner.tab_id.as_deref(),
        Some(expected_owner.as_str())
    );
    assert_eq!(adopted.planner, None);
    let transfer = adopted.owner_history.last().unwrap();
    assert_eq!(
        transfer.from.tab_id.as_deref(),
        Some(retired_owner.as_str())
    );
    assert_eq!(transfer.to.tab_id.as_deref(), Some(expected_owner.as_str()));
    assert_eq!(inherited_parent.as_deref(), Some(expected_owner.as_str()));
}

#[test]
fn transfer_requires_source_team_manager_before_atomic_owner_change() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, source_team, source_owner, source_planner, worker) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&source_owner).unwrap();
    let foreign = mux.create_tab(&workspace_id).unwrap();
    let destination_owner = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&destination_owner).unwrap();
    let destination_team = mux
        .resolve_or_mint_team_for_parent(&destination_owner)
        .unwrap()
        .team_id;
    let plan = store
        .create_plan_for_team(
            "Transfer authorization".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&source_owner),
            source_team,
            vec![],
            1,
        )
        .unwrap();
    store
        .set_planner_ref(
            &plan.id,
            Some(crate::plan::PlannerRef::Tab {
                tab_id: source_planner.clone(),
                session_id: None,
                title: None,
            }),
            2,
        )
        .unwrap();
    let before = store.get(&plan.id).unwrap();
    let destination = crate::plan::OrchestratorRef::from_tab(&destination_owner);

    for caller in [worker, foreign] {
        let error = transfer_team_plan_core(
            &mut mux,
            &store,
            &plan.id,
            destination.clone(),
            "unauthorized",
            TeamMutationCaller::AgentTab(caller),
            3,
        )
        .unwrap_err();
        let TeamPlanError::Team(message) = error else {
            panic!("unauthorized transfer returned a storage error");
        };
        assert!(message.contains("authority"));
        assert!(message.contains("plan rebind authority"));
        assert!(message.contains("old-team member transfers ownership, then new owner adopts"));
        assert!(!message.to_ascii_lowercase().contains("close"));
        assert_eq!(store.get(&plan.id).unwrap(), before);
    }

    let transferred = transfer_team_plan_core(
        &mut mux,
        &store,
        &plan.id,
        destination,
        "authorized handoff",
        TeamMutationCaller::AgentTab(source_owner.clone()),
        4,
    )
    .unwrap();
    assert_eq!(
        transferred.team_id.as_deref(),
        Some(destination_team.as_str())
    );
    assert_eq!(
        transferred.owner.tab_id.as_deref(),
        Some(destination_owner.as_str())
    );
    assert_eq!(
        transferred
            .owner_history
            .last()
            .unwrap()
            .from
            .tab_id
            .as_deref(),
        Some(source_owner.as_str())
    );
    assert_eq!(transferred.planner, None);
    let retired_planner = mux.get_tab(&source_planner).unwrap();
    assert_eq!(retired_planner.role, None);
    assert_eq!(retired_planner.team_id, None);
    assert_eq!(retired_planner.report_target_tab_id, None);
}

#[test]
fn rebind_from_foreign_orchestrator_is_not_circularly_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, source_team, source_owner, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&source_owner).unwrap();
    let destination_owner = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&destination_owner).unwrap();
    let destination_team = mux
        .resolve_or_mint_team_for_parent(&destination_owner)
        .unwrap()
        .team_id;
    let plan = store
        .create_plan_for_team(
            "Foreign destination rebind".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&source_owner),
            source_team,
            vec![],
            1,
        )
        .unwrap();

    let rebound = transfer_team_plan_core(
        &mut mux,
        &store,
        &plan.id,
        crate::plan::OrchestratorRef::from_tab(&destination_owner),
        "destination owner recovery",
        TeamMutationCaller::AgentTab(destination_owner.clone()),
        2,
    )
    .unwrap();

    assert_eq!(
        rebound.owner.tab_id.as_deref(),
        Some(destination_owner.as_str())
    );
    assert_eq!(rebound.team_id.as_deref(), Some(destination_team.as_str()));
}

#[test]
fn adopt_plan_destination_owner_avoids_both_team_gate() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, source_team, source_owner, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&source_owner).unwrap();
    let destination_owner = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&destination_owner).unwrap();
    let destination_team = mux
        .resolve_or_mint_team_for_parent(&destination_owner)
        .unwrap()
        .team_id;
    let plan = store
        .create_plan_for_team(
            "Adopt across Teams".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&source_owner),
            source_team,
            vec![],
            1,
        )
        .unwrap();
    let before = store.get(&plan.id).unwrap();

    let error = adopt_plan_for_team_core(
        &mut mux,
        &store,
        &destination_team,
        &plan.id,
        TeamMutationCaller::AgentTab(source_owner),
        2,
    )
    .unwrap_err();
    let TeamPlanError::Team(message) = error else {
        panic!("destination Team authorization returned a storage error");
    };
    assert!(message.contains("not a member of Team"));
    assert!(message.contains(&destination_team));
    assert!(message.contains("manager authority in destination Team"));
    assert!(message.contains("old-team member transfers ownership, then new owner adopts"));
    assert!(!message.to_ascii_lowercase().contains("close"));
    assert_eq!(store.get(&plan.id).unwrap(), before);

    let adopted = adopt_plan_for_team_core(
        &mut mux,
        &store,
        &destination_team,
        &plan.id,
        TeamMutationCaller::AgentTab(destination_owner.clone()),
        3,
    )
    .unwrap();
    assert_eq!(
        adopted.owner.tab_id.as_deref(),
        Some(destination_owner.as_str())
    );
    assert_eq!(adopted.team_id.as_deref(), Some(destination_team.as_str()));
}

fn spawn_fixture_member(
    mux: &mut MultiplexerState,
    workspace_id: &str,
    parent: &str,
    role: AgentRole,
    profile: Option<&str>,
) -> String {
    let tab_id = mux.create_tab(workspace_id).unwrap();
    mux.set_tab_role_kind(&tab_id, role, profile, Some(parent))
        .unwrap();
    tab_id
}

#[test]
fn bootstrap_add_places_an_anchoring_manager_with_no_caller() {
    let (mux, team_id, _manager, _planner, _worker) = team_members();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        None,
        AddTeamMemberParams {
            provider: "claude".into(),
            role: "orchestrator".into(),
            title: None,
            task: None,
            model: None,
        },
    )
    .expect("an orchestrator anchors, so it needs no caller to parent it");
    let params = prepared.spawn_params;
    assert_eq!(
        params.parent_tab_id, None,
        "an anchoring role takes no parent"
    );
    assert_eq!(
        params.report_target_tab_id, None,
        "there is no caller to report to"
    );
    assert_eq!(
        params.workspace_id.as_deref(),
        mux.get_team(&team_id)
            .map(|team| team.workspace_id.as_str()),
        "the workspace comes from the Team when no caller supplies one"
    );
}

#[test]
fn bootstrap_add_refuses_a_role_that_needs_a_parent() {
    let (mux, team_id, _manager, _planner, _worker) = team_members();
    for role in ["worker", "planner"] {
        let error = prepare_team_member_spawn(
            &mux,
            &team_id,
            None,
            AddTeamMemberParams {
                provider: "claude".into(),
                role: role.into(),
                title: None,
                task: None,
                model: None,
            },
        )
        .expect_err("a parented role cannot be placed without a spawning member");
        assert!(
            error.contains("needs a Team member that can spawn"),
            "the refusal must name what is missing, got: {error}"
        );
    }
}

#[test]
fn add_team_member_uses_existing_manager_and_team() {
    let (mut mux, team_id, manager, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&manager).unwrap();
    let before_count = mux.team_count();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        Some(&manager),
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "worker".into(),
            title: None,
            task: None,
            model: None,
        },
    )
    .unwrap();
    let tab_id = spawn_fixture_member(&mut mux, &workspace_id, &manager, AgentRole::Worker, None);
    let events = crate::api::events::EventBroadcaster::new();
    let result = commit_team_member_spawn(
        &mut mux,
        &events,
        &team_id,
        Some(&manager),
        &tab_id,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .unwrap();
    let tab = mux.get_tab(&result.tab_id).unwrap();
    assert_eq!(tab.parent_tab_id.as_deref(), Some(manager.as_str()));
    assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(tab.role, Some(AgentRole::Worker));
    assert_eq!(
        mux.find_workspace_for_tab(&result.tab_id).as_deref(),
        Some(workspace_id.as_str())
    );
    assert_eq!(mux.team_count(), before_count);
    assert_eq!(result.team.id, team_id);
}

#[test]
fn manual_team_worker_has_system_identity_and_no_starter_turn() {
    let (mux, team_id, manager, _, _) = team_members();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        Some(&manager),
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "worker".into(),
            title: Some("Manual worker".into()),
            task: None,
            model: None,
        },
    )
    .unwrap();

    assert_eq!(prepared.spawn_params.role.as_deref(), Some("Worker"));
    assert_eq!(
        prepared.spawn_params.parent_tab_id.as_deref(),
        Some(manager.as_str())
    );
    assert_eq!(
        prepared.spawn_params.spawned_by_tab_id.as_deref(),
        Some(manager.as_str())
    );
    assert!(prepared.spawn_params.task.is_none());
    assert_eq!(
        prepared.spawn_params.starter_prompt,
        crate::agent::provider::StarterPrompt::NoStarter
    );

    let spec = crate::service::spawn::SpawnTabSpec::from_agent_params(prepared.spawn_params);
    let json = serde_json::to_string(&spec).unwrap();
    let restored: crate::service::spawn::SpawnTabSpec = serde_json::from_str(&json).unwrap();
    let restored = restored.to_agent_params().unwrap();
    let system_context = crate::service::orchestrator::spawn_onboarding_task(&restored).unwrap();

    assert_eq!(restored.role.as_deref(), Some("Worker"));
    assert!(restored.task.is_none());
    assert_eq!(
        restored.starter_prompt,
        crate::agent::provider::StarterPrompt::NoStarter
    );
    assert_eq!(
        system_context.matches("You are a CodeTerm Worker.").count(),
        1
    );
    assert!(!system_context.contains("## Assignment"));
}

#[test]
fn new_explicit_task_worker_gets_exactly_one_assigned_starter() {
    let (mux, team_id, manager, _, _) = team_members();
    let brief = "Audit the Team worker starter cardinality.";
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        Some(&manager),
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "worker".into(),
            title: None,
            task: Some(brief.into()),
            model: None,
        },
    )
    .unwrap();
    let spec = crate::service::spawn::SpawnTabSpec::from_agent_params(prepared.spawn_params);
    let restored = spec.to_agent_params().unwrap();
    let system_context = crate::service::orchestrator::spawn_onboarding_task(&restored).unwrap();

    assert_eq!(restored.role.as_deref(), Some("Worker"));
    assert_eq!(restored.task.as_deref(), Some(brief));
    assert_eq!(
        restored.starter_prompt,
        crate::agent::provider::StarterPrompt::AssignedTask
    );
    assert_eq!(system_context.matches(brief).count(), 1);
    assert_eq!(
        system_context.matches("You are a CodeTerm Worker.").count(),
        1
    );
}

#[test]
fn whitespace_team_task_is_normalized_to_manual_creation() {
    let (mux, team_id, manager, _, _) = team_members();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        Some(&manager),
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "worker".into(),
            title: None,
            task: Some("   ".into()),
            model: None,
        },
    )
    .unwrap();

    assert!(prepared.spawn_params.task.is_none());
    assert_eq!(
        prepared.spawn_params.starter_prompt,
        crate::agent::provider::StarterPrompt::NoStarter
    );
}

#[test]
fn add_team_member_sets_exact_planner_role_and_emits_one_state_change() {
    let (mut mux, team_id, manager, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&manager).unwrap();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        Some(&manager),
        AddTeamMemberParams {
            provider: "spawn-capable".into(),
            role: "planner".into(),
            title: None,
            task: None,
            model: None,
        },
    )
    .unwrap();
    let tab_id = spawn_fixture_member(
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
        Some(&manager),
        &tab_id,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .unwrap();
    let tab = mux.get_tab(&result.tab_id).unwrap();
    assert_eq!(tab.role, Some(AgentRole::Manager));
    assert_eq!(tab.role_profile.as_deref(), Some("planner"));
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
fn failed_plan_takeover_rolls_back_the_replacement_anchor() {
    let (_dir, store) = isolated_plan_store();
    let (mut mux, team_id, orchestrator, _planner, _worker) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let mut plan = crate::plan::Plan::new(
        "Atomic replacement".into(),
        workspace_id.clone(),
        crate::plan::OrchestratorRef::from_tab(&orchestrator),
    );
    plan.team_id = Some(team_id.clone());
    store.save(&mut plan, 1).unwrap();
    let before = store.get(&plan.id).unwrap();
    mux.close_tab(&orchestrator).unwrap();
    let prepared = prepare_team_member_spawn(
        &mux,
        &team_id,
        None,
        AddTeamMemberParams {
            provider: "provider-neutral".into(),
            role: "orchestrator".into(),
            title: None,
            task: None,
            model: None,
        },
    )
    .unwrap();
    let replacement = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&replacement).unwrap();

    let error = commit_team_member_with_plan_takeover(
        &mut mux,
        &EventBroadcaster::new(),
        &team_id,
        None,
        &replacement,
        &prepared.expected_role,
        &prepared.expected_profile,
        true,
        |_| {
            Some(staffing::PlanTakeover {
                plan_id: plan.id.clone(),
                owner_tab_id: replacement.clone(),
                error: Some("forced takeover failure".into()),
            })
        },
    )
    .unwrap_err();

    assert!(error.contains("forced takeover failure"));
    assert!(mux.get_tab(&replacement).is_none());
    assert!(mux.get_team(&team_id).is_some());
    assert!(staffing::live_team_orchestrators(&mux, &team_id).is_empty());
    let after = store.get(&plan.id).unwrap();
    assert_eq!(after.owner, before.owner);
    assert_eq!(after.owner_history, before.owner_history);
    assert_eq!(after.team_id, before.team_id);
    assert_eq!(after.status, before.status);
}

#[test]
fn add_team_member_rejects_worker_foreign_and_unspawnable_without_mutation() {
    let (mut mux, team_id, manager, _, worker) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&manager).unwrap();
    let (worker, foreign) = {
        let foreign = mux.create_tab(&workspace_id).unwrap();
        (worker, foreign)
    };
    let before_tabs = mux.tab_count_in_workspace(&workspace_id);
    let before_teams = mux.team_count();
    for caller in [&worker, &foreign] {
        assert!(prepare_team_member_spawn_with_provider_validation(
            &mux,
            &team_id,
            Some(caller),
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
            Some(&manager),
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
        Some(&manager),
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
    let tab_id = spawn_fixture_member(&mut mux, &workspace_id, &manager, AgentRole::Worker, None);
    mux.get_tab_mut(&tab_id).unwrap().team_id = None;
    let events = crate::api::events::EventBroadcaster::new();
    let mut subscription = events.subscribe();
    assert!(commit_team_member_spawn(
        &mut mux,
        &events,
        &team_id,
        Some(&manager),
        &tab_id,
        &expected.expected_role,
        &expected.expected_profile
    )
    .is_err());
    assert!(mux.get_tab(&tab_id).is_none());
    assert!(matches!(
        subscription.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(mux.tab_count_in_workspace(&workspace_id), before_tabs);
    assert_eq!(mux.team_count(), before_teams);
}

mod close_lifecycle;
