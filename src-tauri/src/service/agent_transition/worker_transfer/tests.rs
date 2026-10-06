use super::*;
use crate::multiplexer::types::TaskBinding;
use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskLaneMetadata};
use crate::service::plan::task_assignment::AssignmentRuntimeBinding;
use crate::service::test_fixture::ServiceFixture;
use crate::service::AuthenticatedPlanCaller;
use codeterm_types::agent::role_kind::builtins;
use codeterm_types::team::TeamLifecycle;

struct TransferFixture {
    fixture: ServiceFixture,
    source: String,
    destination: String,
    worker: String,
    plan_id: String,
    task_id: String,
    attempt_id: String,
}

impl TransferFixture {
    fn new() -> Self {
        let (mut mux, source) = MultiplexerState::new();
        let workspace_id = mux.find_workspace_for_tab(&source).unwrap();
        let destination = mux.create_tab(&workspace_id).unwrap();
        let worker = mux.create_tab(&workspace_id).unwrap();
        arm(&mut mux, &source, "source");
        arm(&mut mux, &destination, "destination");
        arm(&mut mux, &worker, "worker");
        mux.get_tab_mut(&source).unwrap().pty_id = Some("pty-source".into());
        mux.get_tab_mut(&destination).unwrap().pty_id = Some("pty-destination".into());
        mux.get_tab_mut(&worker).unwrap().pty_id = Some("pty-worker".into());
        mux.set_orchestrator_role(&source).unwrap();
        mux.set_orchestrator_role(&destination).unwrap();
        let source_team = mux
            .resolve_or_mint_team_for_parent(&source)
            .unwrap()
            .team_id;
        let destination_team = mux
            .resolve_or_mint_team_for_parent(&destination)
            .unwrap()
            .team_id;
        mux.set_team_lifecycle(&source_team, TeamLifecycle::Active)
            .unwrap();
        mux.set_team_lifecycle(&destination_team, TeamLifecycle::Active)
            .unwrap();
        mux.set_tab_role_kind(
            &worker,
            AgentRole::Worker,
            Some(builtins::WORKER),
            Some(&source),
        )
        .unwrap();
        {
            let tab = mux.get_tab_mut(&worker).unwrap();
            tab.report_target_tab_id = Some(source.clone());
            tab.spawned_by_tab_id = Some("original-spawner".into());
        }

        let fixture = ServiceFixture::new(mux);
        let mut plan = Plan::new(
            "same-plan transfer".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&destination),
        );
        plan.team_id = Some(destination_team);
        plan.mode = PlanMode::Approved;
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let worktree_path = fixture
            .temp
            .path()
            .join("worker-lane")
            .display()
            .to_string();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan.id,
                crate::service::plan::NewTask {
                    name: "preserved worker task".into(),
                    kind: "implement".into(),
                    acceptance_criteria: vec!["retain the acknowledged attempt".into()],
                    lane: TaskLaneMetadata {
                        tab_id: Some(worker.clone()),
                        workspace_id: Some(workspace_id.clone()),
                        worktree_path: Some(worktree_path.clone()),
                        branch: Some("lane/worker-transfer".into()),
                        base: Some("base-transfer".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        fixture
            .service
            .plan_store
            .assign_task_with_runtime_binding(
                &plan.id,
                &task_id,
                &worker,
                None,
                Some("Worker"),
                Some(&AssignmentRuntimeBinding {
                    session_id: "session-worker".into(),
                    session_binding_nonce: "nonce-worker".into(),
                    provider: Some("codex".into()),
                    model: Some("gpt-6-luna".into()),
                    session_model: None,
                    reasoning_effort: Some("xhigh".into()),
                }),
                None,
                3,
            )
            .unwrap();
        let assigned = fixture.service.plan_store.get(&plan.id).unwrap();
        let attempt_id = assigned.tasks[0].execution_attempts[0].id.clone();
        fixture
            .service
            .plan_store
            .acknowledge_task_attempt(&plan.id, &task_id, &worker, &attempt_id, 4)
            .unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&worker).unwrap();
            tab.cwd = Some(worktree_path);
            tab.task_binding = Some(TaskBinding {
                plan_id: Some(plan.id.clone()),
                task_id: Some(task_id.clone()),
                attempt_id: Some(attempt_id.clone()),
                spawn_handle: None,
                bound_at: 2,
            });
        }

        Self {
            fixture,
            source,
            destination,
            worker,
            plan_id: plan.id,
            task_id,
            attempt_id,
        }
    }

    fn caller(&self) -> AuthenticatedPlanCaller {
        caller(&self.source, "source")
    }
}

fn arm(mux: &mut MultiplexerState, tab_id: &str, suffix: &str) {
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.chat_view = true;
    tab.agent_session_id = Some(format!("session-{suffix}"));
    tab.session_binding_nonce = Some(format!("nonce-{suffix}"));
    tab.reported_session_binding_nonce = Some(format!("nonce-{suffix}"));
}

fn caller(tab_id: &str, suffix: &str) -> AuthenticatedPlanCaller {
    AuthenticatedPlanCaller {
        tab_id: Some(tab_id.into()),
        session_id: Some(format!("session-{suffix}")),
        binding_nonce: Some(format!("nonce-{suffix}")),
    }
}

fn assert_refused_without_changes(
    fixture: &TransferFixture,
    call: impl FnOnce(&ServiceContext) -> Result<(), String>,
) {
    let before_mux = mux_snapshot_without_generation(&fixture.fixture.service.mux.read());
    let before_plan = fixture
        .fixture
        .service
        .plan_store
        .get(&fixture.plan_id)
        .unwrap();
    assert!(call(&fixture.fixture.service).is_err());
    assert_eq!(
        mux_snapshot_without_generation(&fixture.fixture.service.mux.read()),
        before_mux,
    );
    assert_eq!(
        fixture
            .fixture
            .service
            .plan_store
            .get(&fixture.plan_id)
            .unwrap(),
        before_plan
    );
}

fn mux_snapshot_without_generation(mux: &MultiplexerState) -> serde_json::Value {
    let mut snapshot = serde_json::to_value(mux.snapshot()).unwrap();
    snapshot.as_object_mut().unwrap().remove("generation");
    snapshot
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_source_manager_transfer_preserves_the_exact_plan_attempt_and_worker_provenance()
{
    let fixture = TransferFixture::new();
    let before_plan = fixture
        .fixture
        .service
        .plan_store
        .get(&fixture.plan_id)
        .unwrap();
    let before_worker = fixture
        .fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.worker)
        .unwrap()
        .clone();
    let outcome = fixture
        .fixture
        .service
        .transfer_task_bound_worker(&fixture.caller(), &fixture.worker, &fixture.destination)
        .unwrap();
    let after_plan = fixture
        .fixture
        .service
        .plan_store
        .get(&fixture.plan_id)
        .unwrap();
    let mux = fixture.fixture.service.mux.read();
    let after_worker = mux.get_tab(&fixture.worker).unwrap();

    assert_eq!(outcome.status, "committed_observation_unconfirmed");
    assert_eq!(outcome.source_manager_tab_id, fixture.source);
    assert_eq!(outcome.destination_manager_tab_id, fixture.destination);
    assert_eq!(outcome.plan_id, fixture.plan_id);
    assert_eq!(outcome.task_id, fixture.task_id);
    assert_eq!(outcome.attempt_id, fixture.attempt_id);
    assert_eq!(after_plan, before_plan);
    assert_eq!(after_worker.role, Some(AgentRole::Worker));
    assert_eq!(after_worker.role_profile.as_deref(), Some(builtins::WORKER));
    assert_eq!(
        after_worker.parent_tab_id.as_deref(),
        Some(fixture.destination.as_str())
    );
    assert_eq!(
        after_worker.team_id,
        mux.get_tab(&fixture.destination).unwrap().team_id
    );
    assert_eq!(
        after_worker.report_target_tab_id.as_deref(),
        Some(fixture.destination.as_str())
    );
    assert_eq!(
        after_worker.report_target_mesh_peer,
        before_worker.report_target_mesh_peer
    );
    assert_eq!(
        after_worker.spawned_by_tab_id,
        before_worker.spawned_by_tab_id
    );
    assert_eq!(
        after_worker.agent_session_id,
        before_worker.agent_session_id
    );
    assert_eq!(
        after_worker.session_binding_nonce,
        before_worker.session_binding_nonce
    );
    assert_eq!(
        after_worker.reported_session_binding_nonce,
        before_worker.reported_session_binding_nonce
    );
    assert_eq!(after_worker.pty_id, before_worker.pty_id);
    assert_eq!(after_worker.cwd, before_worker.cwd);
    assert_eq!(after_worker.task_binding, before_worker.task_binding);
    assert_eq!(after_worker.depth, 2);
    assert!(!after_worker.can_spawn);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_all_generic_service_entrypoints_refuse_task_bound_cross_parent_worker_transfer()
{
    let fixture = TransferFixture::new();
    assert_refused_without_changes(&fixture, |service| {
        service.set_tab_role(&fixture.worker, "Worker", Some(&fixture.destination), None)
    });

    let fixture = TransferFixture::new();
    assert_refused_without_changes(&fixture, |service| {
        service.set_tab_role_with_profile(
            &fixture.worker,
            "Worker",
            None,
            Some(&fixture.destination),
            None,
        )
    });

    let fixture = TransferFixture::new();
    assert_refused_without_changes(&fixture, |service| {
        service
            .transition_agent(AgentTransitionRequest {
                tab_id: fixture.worker.clone(),
                destination: AgentPlacement::Worker {
                    parent_tab_id: fixture.destination.clone(),
                    role_profile: None,
                },
                plan_binding: None,
                require_verified_session: true,
                planner_replacement: None,
                plan_rebind: None,
            })
            .map(|_| ())
    });
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_transfer_requires_source_manager_authority_and_the_existing_plan_owner_team() {
    let fixture = TransferFixture::new();
    for invalid in [
        AuthenticatedPlanCaller::default(),
        caller(&fixture.source, "wrong-session"),
        caller(&fixture.destination, "destination"),
        caller(&fixture.worker, "worker"),
    ] {
        assert_refused_without_changes(&fixture, |service| {
            service
                .transfer_task_bound_worker(&invalid, &fixture.worker, &fixture.destination)
                .map(|_| ())
        });
    }

    let fixture = TransferFixture::new();
    let source_team = fixture
        .fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.source)
        .unwrap()
        .team_id
        .clone()
        .unwrap();
    fixture
        .fixture
        .service
        .plan_store
        .with_plan(&fixture.plan_id, 5, |plan| {
            plan.team_id = Some(source_team);
            Ok(())
        })
        .unwrap();
    assert_refused_without_changes(&fixture, |service| {
        service
            .transfer_task_bound_worker(&fixture.caller(), &fixture.worker, &fixture.destination)
            .map(|_| ())
    });
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_transfer_refuses_stale_worker_nonce_without_mutating_placement_or_task() {
    let fixture = TransferFixture::new();
    fixture
        .fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.worker)
        .unwrap()
        .reported_session_binding_nonce = Some("stale-worker-nonce".into());
    let caller = fixture.caller();
    let before_plan = fixture
        .fixture
        .service
        .plan_store
        .get(&fixture.plan_id)
        .unwrap();
    let before_worker = fixture
        .fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.worker)
        .unwrap()
        .clone();
    assert!(fixture
        .fixture
        .service
        .transfer_task_bound_worker(&caller, &fixture.worker, &fixture.destination)
        .is_err());
    assert_eq!(
        fixture
            .fixture
            .service
            .plan_store
            .get(&fixture.plan_id)
            .unwrap(),
        before_plan
    );
    let mux = fixture.fixture.service.mux.read();
    let after = mux.get_tab(&fixture.worker).unwrap();
    assert_eq!(after.parent_tab_id, before_worker.parent_tab_id);
    assert_eq!(after.team_id, before_worker.team_id);
    assert_eq!(after.task_binding, before_worker.task_binding);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_transfer_rechecks_source_authority_and_accepts_current_destination_rotation() {
    let fixture = TransferFixture::new();
    assert!(fixture
        .fixture
        .service
        .task_bound_worker_transfer_candidate(&fixture.worker, &fixture.destination));
    {
        let mut mux = fixture.fixture.service.mux.write();
        let source = mux.get_tab_mut(&fixture.source).unwrap();
        source.session_binding_nonce = Some("nonce-source-next".into());
        source.reported_session_binding_nonce = Some("nonce-source-next".into());
    }
    assert_refused_without_changes(&fixture, |service| {
        service
            .transfer_task_bound_worker(&fixture.caller(), &fixture.worker, &fixture.destination)
            .map(|_| ())
    });

    let fixture = TransferFixture::new();
    assert!(fixture
        .fixture
        .service
        .task_bound_worker_transfer_candidate(&fixture.worker, &fixture.destination));
    let before_plan = fixture
        .fixture
        .service
        .plan_store
        .get(&fixture.plan_id)
        .unwrap();
    assert!(before_plan.owner.session_id.is_none());
    {
        let mut mux = fixture.fixture.service.mux.write();
        let destination = mux.get_tab_mut(&fixture.destination).unwrap();
        destination.session_binding_nonce = Some("nonce-destination-next".into());
        destination.reported_session_binding_nonce = Some("nonce-destination-next".into());
    }
    let outcome = fixture
        .fixture
        .service
        .transfer_task_bound_worker(&fixture.caller(), &fixture.worker, &fixture.destination)
        .unwrap();
    assert_eq!(outcome.status, "committed_observation_unconfirmed");
    assert_eq!(
        fixture
            .fixture
            .service
            .plan_store
            .get(&fixture.plan_id)
            .unwrap(),
        before_plan
    );
    assert_eq!(
        fixture
            .fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.destination)
            .unwrap()
            .session_binding_nonce
            .as_deref(),
        Some("nonce-destination-next")
    );

    let fixture = TransferFixture::new();
    assert!(fixture
        .fixture
        .service
        .task_bound_worker_transfer_candidate(&fixture.worker, &fixture.destination));
    {
        let mut mux = fixture.fixture.service.mux.write();
        mux.get_tab_mut(&fixture.destination)
            .unwrap()
            .session_binding_nonce = Some("nonce-destination-next".into());
    }
    assert_refused_without_changes(&fixture, |service| {
        service
            .transfer_task_bound_worker(&fixture.caller(), &fixture.worker, &fixture.destination)
            .map(|_| ())
    });
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_transfer_refuses_a_new_child_or_remote_report_target_before_commit() {
    let fixture = TransferFixture::new();
    {
        let mut mux = fixture.fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&fixture.worker).unwrap();
        let child = mux.create_tab(&workspace).unwrap();
        mux.get_tab_mut(&child).unwrap().parent_tab_id = Some(fixture.worker.clone());
    }
    assert_refused_without_changes(&fixture, |service| {
        service
            .transfer_task_bound_worker(&fixture.caller(), &fixture.worker, &fixture.destination)
            .map(|_| ())
    });

    let fixture = TransferFixture::new();
    fixture
        .fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.worker)
        .unwrap()
        .report_target_mesh_peer = Some("remote-peer".into());
    assert_refused_without_changes(&fixture, |service| {
        service
            .transfer_task_bound_worker(&fixture.caller(), &fixture.worker, &fixture.destination)
            .map(|_| ())
    });
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_same_parent_transfer_keeps_the_existing_task_binding_contract() {
    let fixture = TransferFixture::new();
    let source_team = fixture
        .fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.source)
        .unwrap()
        .team_id
        .clone()
        .unwrap();
    fixture
        .fixture
        .service
        .plan_store
        .with_plan(&fixture.plan_id, 5, |plan| {
            plan.owner = OrchestratorRef::from_tab(&fixture.source);
            plan.team_id = Some(source_team);
            Ok(())
        })
        .unwrap();
    let before = fixture
        .fixture
        .service
        .plan_store
        .get(&fixture.plan_id)
        .unwrap();

    fixture
        .fixture
        .service
        .transition_agent(AgentTransitionRequest {
            tab_id: fixture.worker.clone(),
            destination: AgentPlacement::Worker {
                parent_tab_id: fixture.source.clone(),
                role_profile: Some(builtins::WORKER.into()),
            },
            plan_binding: None,
            require_verified_session: true,
            planner_replacement: None,
            plan_rebind: None,
        })
        .unwrap();

    assert_eq!(
        fixture
            .fixture
            .service
            .plan_store
            .get(&fixture.plan_id)
            .unwrap(),
        before
    );
    let mux = fixture.fixture.service.mux.read();
    assert_eq!(
        mux.get_tab(&fixture.worker)
            .unwrap()
            .parent_tab_id
            .as_deref(),
        Some(fixture.source.as_str())
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn worker_transfer_unbound_worker_cross_parent_transition_keeps_the_existing_service_contract() {
    let (mut mux, source) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&source).unwrap();
    let destination = mux.create_tab(&workspace_id).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.get_tab_mut(&source).unwrap().pty_id = Some("pty-source".into());
    mux.get_tab_mut(&destination).unwrap().pty_id = Some("pty-destination".into());
    mux.get_tab_mut(&worker).unwrap().pty_id = Some("pty-worker".into());
    mux.set_orchestrator_role(&source).unwrap();
    mux.set_orchestrator_role(&destination).unwrap();
    mux.resolve_or_mint_team_for_parent(&source).unwrap();
    mux.resolve_or_mint_team_for_parent(&destination).unwrap();
    mux.set_tab_role_kind(
        &worker,
        AgentRole::Worker,
        Some(builtins::WORKER),
        Some(&source),
    )
    .unwrap();
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .transition_agent(AgentTransitionRequest {
            tab_id: worker.clone(),
            destination: AgentPlacement::Worker {
                parent_tab_id: destination.clone(),
                role_profile: Some(builtins::WORKER.to_string()),
            },
            plan_binding: None,
            require_verified_session: false,
            planner_replacement: None,
            plan_rebind: None,
        })
        .unwrap();

    let mux = fixture.service.mux.read();
    let moved_worker = mux.get_tab(&worker).unwrap();
    assert_eq!(
        moved_worker.parent_tab_id.as_deref(),
        Some(destination.as_str())
    );
    assert_eq!(moved_worker.role_profile.as_deref(), Some(builtins::WORKER));
}
