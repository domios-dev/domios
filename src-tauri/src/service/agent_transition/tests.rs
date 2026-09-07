use super::*;
use crate::multiplexer::types::Tab;
use crate::plan::{OrchestratorRef, Plan};
use std::sync::Arc;

struct Fixture {
    mux: MultiplexerState,
    store: Arc<crate::service::plan::PlanStore>,
    owner: String,
    other_owner: String,
    target: String,
    plan_id: String,
}

impl Fixture {
    fn new(root: &std::path::Path) -> Self {
        let store = Arc::new(crate::service::plan::PlanStore::new_for_test(root));
        let (mut mux, owner) = MultiplexerState::new();
        let workspace_id = mux.find_workspace_for_tab(&owner).unwrap();
        let other_owner = mux.create_tab(&workspace_id).unwrap();
        let target = mux.create_tab(&workspace_id).unwrap();
        arm_session(mux.get_tab_mut(&owner).unwrap(), "owner");
        arm_session(mux.get_tab_mut(&other_owner).unwrap(), "other-owner");
        arm_session(mux.get_tab_mut(&target).unwrap(), "target");
        mux.set_orchestrator_role(&owner).unwrap();
        mux.set_orchestrator_role(&other_owner).unwrap();
        mux.resolve_or_mint_team_for_parent(&owner).unwrap();
        mux.resolve_or_mint_team_for_parent(&other_owner).unwrap();
        let mut plan = Plan::new(
            "Lifecycle".into(),
            root.display().to_string(),
            OrchestratorRef::from_tab(&owner),
        );
        plan.team_id = mux.get_tab(&owner).unwrap().team_id.clone();
        store.save(&mut plan, 1).unwrap();
        Self {
            mux,
            store,
            owner,
            other_owner,
            target,
            plan_id: plan.id,
        }
    }

    fn transition(&mut self, destination: AgentPlacement) {
        self.transition_bound(destination, None)
    }

    fn transition_bound(&mut self, destination: AgentPlacement, plan_binding: Option<String>) {
        execute_transition(
            &mut self.mux,
            &self.store,
            AgentTransitionRequest {
                tab_id: self.target.clone(),
                destination,
                plan_binding,
                require_verified_session: true,
                planner_replacement: None,
                plan_rebind: None,
            },
            TransitionFailurePoint::None,
        )
        .unwrap();
    }

    fn target(&self) -> &Tab {
        self.mux.get_tab(&self.target).unwrap()
    }
}

fn arm_session(tab: &mut Tab, suffix: &str) {
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some(format!("session-{suffix}"));
    tab.session_binding_nonce = Some(format!("nonce-{suffix}"));
    tab.reported_session_binding_nonce = Some(format!("nonce-{suffix}"));
}

fn planner_placement() -> AgentPlacement {
    AgentPlacement::Manager {
        role_profile: "planner".to_string(),
        parent_tab_id: None,
        can_spawn: None,
    }
}

fn effective_role(tab: &Tab) -> &'static str {
    match (tab.role, tab.role_profile.as_deref()) {
        (Some(AgentRole::Manager), Some("planner")) => "Planner",
        (Some(AgentRole::Manager), Some("orchestrator")) => "Orchestrator",
        (Some(AgentRole::Manager), _) => "Manager",
        (Some(AgentRole::Worker), _) => "Worker",
        _ => "Roleless",
    }
}

fn mux_state_without_generation(mux: &MultiplexerState) -> serde_json::Value {
    let mut value = serde_json::to_value(mux.snapshot()).unwrap();
    value.as_object_mut().unwrap().remove("generation");
    value
}

fn assert_planner_tuple(fixture: &Fixture, owner: &str) {
    let tab = fixture.target();
    let owner_tab = fixture.mux.get_tab(owner).unwrap();
    assert_eq!(effective_role(tab), "Planner");
    assert_eq!(tab.parent_tab_id.as_deref(), Some(owner));
    assert_eq!(tab.team_id, owner_tab.team_id);
    assert!(!tab.can_spawn);
    assert_eq!(tab.report_target_tab_id.as_deref(), Some(owner));
    assert_eq!(
        tab.session_binding_nonce,
        tab.reported_session_binding_nonce
    );
    assert_eq!(
        fixture.mux.get_children_ids(&fixture.target),
        Vec::<String>::new()
    );
    let plan = fixture.store.get(&fixture.plan_id).unwrap();
    assert_eq!(plan.owner.tab_id.as_deref(), Some(owner));
    assert_eq!(plan.team_id, tab.team_id);
    assert!(matches!(
        plan.planner,
        Some(PlannerRef::Tab {
            tab_id,
            session_id: Some(session_id),
            ..
        }) if tab_id == fixture.target && session_id == "session-target"
    ));
}

fn assert_orchestrator_tuple(fixture: &Fixture) {
    let tab = fixture.target();
    assert_eq!(effective_role(tab), "Orchestrator");
    assert_eq!(tab.parent_tab_id, None);
    assert!(tab.can_spawn);
    assert_eq!(tab.report_target_tab_id, None);
    assert_eq!(tab.depth, 1);
    assert_eq!(
        tab.session_binding_nonce,
        tab.reported_session_binding_nonce
    );
    assert!(!fixture.store.list_full().iter().any(|plan| {
        matches!(
            &plan.planner,
            Some(PlannerRef::Tab { tab_id, .. }) if tab_id == &fixture.target
        )
    }));
}

fn assert_worker_tuple(fixture: &Fixture, owner: &str) {
    let tab = fixture.target();
    let owner_tab = fixture.mux.get_tab(owner).unwrap();
    assert_eq!(effective_role(tab), "Worker");
    assert_eq!(tab.parent_tab_id.as_deref(), Some(owner));
    assert_eq!(tab.team_id, owner_tab.team_id);
    assert!(!tab.can_spawn);
    assert_eq!(tab.report_target_tab_id.as_deref(), Some(owner));
    assert_eq!(tab.depth, 2);
    assert_eq!(
        tab.session_binding_nonce,
        tab.reported_session_binding_nonce
    );
}

#[test]
fn agent_transition_matrix_oracle() {
    let dir = tempfile::tempdir().unwrap();

    let mut orchestrator_to_planner = Fixture::new(dir.path());
    orchestrator_to_planner
        .mux
        .set_orchestrator_role(&orchestrator_to_planner.target)
        .unwrap();
    let workspace_id = orchestrator_to_planner
        .mux
        .find_workspace_for_tab(&orchestrator_to_planner.target)
        .unwrap();
    let child = orchestrator_to_planner
        .mux
        .create_tab(&workspace_id)
        .unwrap();
    orchestrator_to_planner
        .mux
        .set_tab_role(
            &child,
            AgentRole::Worker,
            Some(&orchestrator_to_planner.target),
        )
        .unwrap();
    let child_task = orchestrator_to_planner
        .store
        .add_task(
            &orchestrator_to_planner.plan_id,
            crate::service::plan::NewTask {
                name: "Child binding".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    orchestrator_to_planner
        .store
        .assign_task(
            &orchestrator_to_planner.plan_id,
            &child_task,
            &child,
            None,
            3,
        )
        .unwrap();
    orchestrator_to_planner.transition_bound(
        planner_placement(),
        Some(orchestrator_to_planner.plan_id.clone()),
    );
    assert_planner_tuple(&orchestrator_to_planner, &orchestrator_to_planner.owner);
    let orphan = orchestrator_to_planner.mux.get_tab(&child).unwrap();
    assert_eq!(orphan.role, None);
    assert_eq!(orphan.parent_tab_id, None);
    assert_eq!(orphan.team_id, None);
    let child_task = orchestrator_to_planner
        .store
        .get(&orchestrator_to_planner.plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == child_task)
        .unwrap();
    assert_eq!(child_task.assigned_tab_id, None);
    assert_eq!(child_task.lane.tab_id, None);

    let mut worker_to_planner = Fixture::new(dir.path());
    worker_to_planner
        .mux
        .set_tab_role(
            &worker_to_planner.target,
            AgentRole::Worker,
            Some(&worker_to_planner.other_owner),
        )
        .unwrap();
    let task_id = worker_to_planner
        .store
        .add_task(
            &worker_to_planner.plan_id,
            crate::service::plan::NewTask {
                name: "Old worker binding".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    worker_to_planner
        .store
        .assign_task(
            &worker_to_planner.plan_id,
            &task_id,
            &worker_to_planner.target,
            None,
            3,
        )
        .unwrap();
    worker_to_planner
        .transition_bound(planner_placement(), Some(worker_to_planner.plan_id.clone()));
    assert_planner_tuple(&worker_to_planner, &worker_to_planner.other_owner);
    let task = worker_to_planner
        .store
        .get(&worker_to_planner.plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.assigned_tab_id, None);
    assert_eq!(task.lane.tab_id, None);

    let mut planner_to_orchestrator = Fixture::new(dir.path());
    planner_to_orchestrator.transition_bound(
        planner_placement(),
        Some(planner_to_orchestrator.plan_id.clone()),
    );
    planner_to_orchestrator.transition(AgentPlacement::orchestrator(Some(true)));
    assert_orchestrator_tuple(&planner_to_orchestrator);

    let mut worker_reparent = Fixture::new(dir.path());
    worker_reparent
        .mux
        .set_tab_role(
            &worker_reparent.target,
            AgentRole::Worker,
            Some(&worker_reparent.owner),
        )
        .unwrap();
    worker_reparent.transition(AgentPlacement::Worker {
        parent_tab_id: worker_reparent.other_owner.clone(),
        role_profile: None,
    });
    assert_worker_tuple(&worker_reparent, &worker_reparent.other_owner);

    let mut roleless_to_worker = Fixture::new(dir.path());
    roleless_to_worker.transition(AgentPlacement::Worker {
        parent_tab_id: roleless_to_worker.owner.clone(),
        role_profile: None,
    });
    assert_worker_tuple(&roleless_to_worker, &roleless_to_worker.owner);

    let mut roleless_to_planner = Fixture::new(dir.path());
    roleless_to_planner.transition_bound(
        planner_placement(),
        Some(roleless_to_planner.plan_id.clone()),
    );
    assert_planner_tuple(&roleless_to_planner, &roleless_to_planner.owner);

    let mut roleless_to_orchestrator = Fixture::new(dir.path());
    roleless_to_orchestrator.transition(AgentPlacement::orchestrator(Some(true)));
    assert_orchestrator_tuple(&roleless_to_orchestrator);

    let mut planner_to_roleless = Fixture::new(dir.path());
    planner_to_roleless.transition_bound(
        planner_placement(),
        Some(planner_to_roleless.plan_id.clone()),
    );
    planner_to_roleless.transition(AgentPlacement::Roleless);
    let tab = planner_to_roleless.target();
    assert_eq!(effective_role(tab), "Roleless");
    assert_eq!(tab.parent_tab_id, None);
    assert_eq!(tab.team_id, None);
    assert!(!tab.can_spawn);
    assert_eq!(tab.report_target_tab_id, None);
    assert_eq!(
        tab.session_binding_nonce,
        tab.reported_session_binding_nonce
    );
    assert_eq!(
        planner_to_roleless
            .store
            .get(&planner_to_roleless.plan_id)
            .unwrap()
            .planner,
        None
    );
}

#[test]
fn regression_orchestrator_to_planner_single_command_sets_role_and_report_owner() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path());
    fixture.mux.set_orchestrator_role(&fixture.target).unwrap();
    fixture
        .mux
        .get_tab_mut(&fixture.target)
        .unwrap()
        .report_target_tab_id = Some("stale-owner".into());
    fixture
        .store
        .set_planner_ref(
            &fixture.plan_id,
            Some(PlannerRef::Tab {
                tab_id: fixture.target.clone(),
                session_id: None,
                title: None,
            }),
            2,
        )
        .unwrap();

    fixture.transition_bound(planner_placement(), Some(fixture.plan_id.clone()));

    assert_planner_tuple(&fixture, &fixture.owner);
    assert_eq!(effective_role(fixture.target()), "Planner");
    assert_eq!(
        fixture.target().report_target_tab_id.as_deref(),
        Some(fixture.owner.as_str())
    );
}

#[test]
fn rebind_moves_owner_and_team_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path());
    fixture
        .mux
        .set_tab_role(
            &fixture.target,
            AgentRole::Worker,
            Some(&fixture.other_owner),
        )
        .unwrap();
    let old_team = fixture
        .mux
        .get_tab(&fixture.owner)
        .unwrap()
        .team_id
        .clone()
        .unwrap();
    let destination_team = fixture
        .mux
        .get_tab(&fixture.other_owner)
        .unwrap()
        .team_id
        .clone()
        .unwrap();

    fixture.transition_bound(planner_placement(), Some(fixture.plan_id.clone()));

    assert_planner_tuple(&fixture, &fixture.other_owner);
    let plan = fixture.store.get(&fixture.plan_id).unwrap();
    assert_eq!(
        plan.owner.tab_id.as_deref(),
        Some(fixture.other_owner.as_str())
    );
    assert_eq!(plan.team_id.as_deref(), Some(destination_team.as_str()));
    assert!(!fixture
        .store
        .list_full()
        .iter()
        .any(|candidate| candidate.id == fixture.plan_id
            && candidate.team_id.as_deref() == Some(old_team.as_str())));
}

#[test]
fn stale_or_foreign_nonce_rejects_transition_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path());
    fixture
        .mux
        .get_tab_mut(&fixture.target)
        .unwrap()
        .reported_session_binding_nonce = Some("foreign".into());
    let before = mux_state_without_generation(&fixture.mux);
    let before_plan = fixture.store.get(&fixture.plan_id).unwrap();

    let error = execute_transition(
        &mut fixture.mux,
        &fixture.store,
        AgentTransitionRequest {
            tab_id: fixture.target.clone(),
            destination: planner_placement(),
            plan_binding: Some(fixture.plan_id.clone()),
            require_verified_session: true,
            planner_replacement: None,
            plan_rebind: None,
        },
        TransitionFailurePoint::None,
    )
    .unwrap_err();

    assert!(error.contains("stale or foreign"));
    assert_eq!(mux_state_without_generation(&fixture.mux), before);
    assert_eq!(fixture.store.get(&fixture.plan_id).unwrap(), before_plan);
}

#[test]
fn planner_waiver_revokes_live_planner_placement_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path());
    fixture.transition_bound(planner_placement(), Some(fixture.plan_id.clone()));

    execute_transition(
        &mut fixture.mux,
        &fixture.store,
        AgentTransitionRequest {
            tab_id: fixture.target.clone(),
            destination: AgentPlacement::Roleless,
            plan_binding: None,
            require_verified_session: false,
            planner_replacement: Some((
                fixture.plan_id.clone(),
                Some(PlannerRef::Waived {
                    note: "Intentional solo execution".into(),
                }),
            )),
            plan_rebind: None,
        },
        TransitionFailurePoint::None,
    )
    .unwrap();

    assert_eq!(effective_role(fixture.target()), "Roleless");
    assert_eq!(fixture.target().parent_tab_id, None);
    assert_eq!(fixture.target().team_id, None);
    assert_eq!(fixture.target().report_target_tab_id, None);
    assert!(matches!(
        fixture.store.get(&fixture.plan_id).unwrap().planner,
        Some(PlannerRef::Waived { .. })
    ));
}

#[test]
fn final_orchestrator_unlink_retains_team_and_plan_for_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, owner) = MultiplexerState::new();
    arm_session(mux.get_tab_mut(&owner).unwrap(), "owner");
    mux.set_orchestrator_role(&owner).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&owner)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&owner).unwrap();
    let mut plan = Plan::new(
        "Recovery continuity".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner),
    );
    plan.team_id = Some(team_id.clone());
    store.save(&mut plan, 1).unwrap();

    execute_transition_at(
        &mut mux,
        &store,
        AgentTransitionRequest::roleless(owner.clone()),
        TransitionFailurePoint::None,
        2,
    )
    .unwrap();

    assert_eq!(mux.list_teams().len(), 1);
    assert!(mux.get_team(&team_id).is_some());
    let retired = store.get(&plan.id).unwrap();
    assert_eq!(retired.id, plan.id);
    assert_eq!(retired.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(retired.status, crate::plan::PlanStatus::Active);
    assert_eq!(retired.owner.tab_id, None);
    let retirement = retired.owner_history.last().unwrap();
    assert_eq!(retirement.from.tab_id.as_deref(), Some(owner.as_str()));
    assert_eq!(retirement.to.tab_id, None);

    let prepared = crate::service::team::prepare_team_member_spawn_with_provider_validation(
        &mux,
        &team_id,
        None,
        crate::service::team::AddTeamMemberParams {
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
    crate::service::team::commit_team_member_spawn(
        &mut mux,
        &crate::api::events::EventBroadcaster::new(),
        &team_id,
        None,
        &replacement,
        &prepared.expected_role,
        &prepared.expected_profile,
    )
    .unwrap();
    let takeover = crate::service::team::take_over_team_plan(
        &mut mux,
        &store,
        &team_id,
        &replacement,
        3,
    )
    .unwrap();

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
    assert_eq!(recovered.owner.tab_id.as_deref(), Some(replacement.as_str()));
    assert_eq!(recovered.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(recovered.status, crate::plan::PlanStatus::Active);
    assert_eq!(recovered.owner_history.len(), 2);
}

#[test]
fn rebind_rolls_back_on_injected_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path());
    fixture
        .mux
        .set_tab_role(
            &fixture.target,
            AgentRole::Worker,
            Some(&fixture.other_owner),
        )
        .unwrap();
    fixture
        .mux
        .get_tab_mut(&fixture.target)
        .unwrap()
        .report_target_tab_id = Some("stale-owner".into());
    let before_mux = mux_state_without_generation(&fixture.mux);
    let before_plans = fixture.store.list_full();

    let error = execute_transition(
        &mut fixture.mux,
        &fixture.store,
        AgentTransitionRequest {
            tab_id: fixture.target.clone(),
            destination: planner_placement(),
            plan_binding: Some(fixture.plan_id.clone()),
            require_verified_session: true,
            planner_replacement: None,
            plan_rebind: None,
        },
        TransitionFailurePoint::AfterPlanCommit,
    )
    .unwrap_err();

    assert!(error.contains("injected"));
    assert_eq!(mux_state_without_generation(&fixture.mux), before_mux);
    assert_eq!(fixture.store.list_full(), before_plans);
}

#[test]
fn rebind_is_idempotent_and_single_owner() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path());
    fixture
        .mux
        .set_tab_role(
            &fixture.target,
            AgentRole::Worker,
            Some(&fixture.other_owner),
        )
        .unwrap();
    let destination_owner = fixture.other_owner.clone();
    let mux = Arc::new(parking_lot::Mutex::new(fixture.mux));
    let store = fixture.store.clone();
    let target = fixture.target.clone();
    let plan_id = fixture.plan_id.clone();
    let mut threads = Vec::new();

    for _ in 0..2 {
        let mux = mux.clone();
        let store = store.clone();
        let target = target.clone();
        let plan_id = plan_id.clone();
        threads.push(std::thread::spawn(move || {
            execute_transition(
                &mut mux.lock(),
                &store,
                AgentTransitionRequest {
                    tab_id: target,
                    destination: planner_placement(),
                    plan_binding: Some(plan_id),
                    require_verified_session: true,
                    planner_replacement: None,
                    plan_rebind: None,
                },
                TransitionFailurePoint::None,
            )
        }));
    }
    for thread in threads {
        thread.join().unwrap().unwrap();
    }

    let plans = store.list_full();
    let bindings = plans
        .iter()
        .filter(|plan| {
            matches!(
                &plan.planner,
                Some(PlannerRef::Tab { tab_id, .. }) if tab_id == &target
            )
        })
        .count();
    assert_eq!(bindings, 1);
    assert_eq!(
        plans
            .iter()
            .filter(|plan| plan.owner.tab_id.as_deref() == Some(destination_owner.as_str()))
            .count(),
        1
    );
    let mux = mux.lock();
    assert_eq!(effective_role(mux.get_tab(&target).unwrap()), "Planner");
    assert_eq!(
        mux.get_children_ids(&destination_owner)
            .into_iter()
            .filter(|tab_id| tab_id == &target)
            .count(),
        1
    );
}

#[test]
fn transfer_reporting_success_without_moving_ownership_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(dir.path());
    let before_mux = mux_state_without_generation(&fixture.mux);
    let before_plan = fixture.store.get(&fixture.plan_id).unwrap();
    let destination_team = fixture
        .mux
        .get_tab(&fixture.other_owner)
        .unwrap()
        .team_id
        .clone()
        .unwrap();

    let error = execute_transition(
        &mut fixture.mux,
        &fixture.store,
        AgentTransitionRequest {
            tab_id: fixture.other_owner.clone(),
            destination: AgentPlacement::orchestrator(Some(true)),
            plan_binding: None,
            require_verified_session: false,
            planner_replacement: None,
            plan_rebind: Some(AgentLifecyclePlanRebind {
                plan_id: fixture.plan_id.clone(),
                owner: OrchestratorRef::from_tab(&fixture.other_owner),
                team_id: destination_team,
                reason: "forced no-op transfer".into(),
                activate: false,
                clear_planner: true,
            }),
        },
        TransitionFailurePoint::SkipPlanRebind,
    )
    .unwrap_err();

    assert!(error.contains("post-condition failed"));
    assert_eq!(fixture.store.get(&fixture.plan_id).unwrap(), before_plan);
    assert_eq!(mux_state_without_generation(&fixture.mux), before_mux);
}

/// No `--parent-tab`: resolves the plan's live Orchestrator owner instead
/// of refusing.
#[test]
fn planner_parent_resolves_from_plan_owner() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(dir.path());

    let resolved = resolve_planner_parent(&fixture.store, &fixture.mux, &fixture.plan_id, None)
        .expect("a plan with a live Orchestrator owner must resolve, not refuse");

    assert_eq!(resolved, fixture.owner);
}

/// An explicit `--parent-tab` still wins over the plan's owner when given.
#[test]
fn planner_parent_explicit_override_wins_over_plan_owner() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(dir.path());

    let resolved = resolve_planner_parent(
        &fixture.store,
        &fixture.mux,
        &fixture.plan_id,
        Some(fixture.other_owner.as_str()),
    )
    .unwrap();

    assert_eq!(resolved, fixture.other_owner);
}

/// A plan with no resolvable Orchestrator owner is refused with a reason
/// naming the plan, never the generic "requires a parent_tab_id".
#[test]
fn planner_parent_refuses_with_plan_naming_reason_when_owner_unresolvable() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(dir.path());
    let mut orphan_plan = Plan::new(
        "Orphaned".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("dead-orchestrator-tab"),
    );
    fixture.store.save(&mut orphan_plan, 1).unwrap();

    let error =
        resolve_planner_parent(&fixture.store, &fixture.mux, &orphan_plan.id, None).unwrap_err();

    assert!(error.contains(&orphan_plan.id), "{error}");
    assert!(!error.contains("requires a parent_tab_id"), "{error}");
}

/// The structural rule is not weakened: `planner` still genuinely requires a
/// parent — it is now satisfied from the plan already supplied, not skipped.
#[test]
fn planner_role_kind_still_requires_a_parent() {
    assert!(role_kind::requires_parent(
        Some(AgentRole::Manager),
        Some(builtins::PLANNER)
    ));
}
