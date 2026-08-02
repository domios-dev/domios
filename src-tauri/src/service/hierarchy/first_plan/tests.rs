use std::sync::Arc;

use crate::lock_hold::TimedRwLock;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{Goal, OrchestratorRef, PlannerRef};
use crate::service::hierarchy::first_plan::{
    create_first_plan_for_caller, create_first_plan_for_orchestrator, FirstPlanCaller,
};
use crate::service::plan::{PlanError, PlanStore};
use crate::service::FirstPlanTeamError;

struct FirstPlanHarness {
    mux: MuxState,
    plan_store: Arc<PlanStore>,
    root: std::path::PathBuf,
}

impl FirstPlanHarness {
    fn new(root: &std::path::Path) -> Self {
        let (state, _) = MultiplexerState::new();
        Self {
            mux: Arc::new(TimedRwLock::new("mux", state)),
            plan_store: Arc::new(PlanStore::new_for_test(root)),
            root: root.to_path_buf(),
        }
    }
}

fn teamless_orchestrator(mux: &MuxState) -> String {
    let mut mux = mux.write();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let owner_tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&owner_tab_id, AgentRole::Orchestrator, None)
        .unwrap();
    owner_tab_id
}

fn bound_pair(mux: &MuxState, suffix: &str) -> (String, String, FirstPlanCaller) {
    let mut mux = mux.write();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let orchestrator = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&orchestrator, AgentRole::Orchestrator, None)
        .unwrap();
    let planner = mux.create_tab(&workspace_id).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    {
        let owner = mux.get_tab_mut(&orchestrator).unwrap();
        owner.agent_session_id = Some(format!("orch-session-{suffix}"));
        owner.pending_paired_planner_tab_id = Some(planner.clone());
    }
    {
        let tab = mux.get_tab_mut(&planner).unwrap();
        tab.parent_tab_id = Some(orchestrator.clone());
        tab.team_id = Some(team_id);
        tab.agent_session_id = Some(format!("planner-session-{suffix}"));
        tab.session_binding_nonce = Some(format!("nonce-{suffix}"));
        tab.reported_session_binding_nonce = Some(format!("nonce-{suffix}"));
    }
    let caller = FirstPlanCaller {
        tab_id: Some(planner.clone()),
        session_id: Some(format!("planner-session-{suffix}")),
        binding_nonce: Some(format!("nonce-{suffix}")),
    };
    (orchestrator, planner, caller)
}

fn planner_create(
    harness: &FirstPlanHarness,
    caller: FirstPlanCaller,
    title: &str,
    now_ms: u64,
) -> Result<super::FirstPlanTeamOutcome, FirstPlanTeamError> {
    create_first_plan_for_caller(
        &harness.mux,
        &harness.plan_store,
        title.into(),
        harness.root.display().to_string(),
        OrchestratorRef::from_tab("untrusted-request-owner"),
        caller,
        vec![],
        None,
        now_ms,
    )
}

#[test]
fn own_team_planner_can_create_plan_without_orchestrator_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let harness = FirstPlanHarness::new(dir.path());
    let (orchestrator, planner, caller) = bound_pair(&harness.mux, "own");
    let outcome = planner_create(&harness, caller, "Planner-authored", 1).unwrap();
    assert_eq!(harness.plan_store.list_index().len(), 1);
    assert_eq!(
        outcome.plan.owner.tab_id.as_deref(),
        Some(orchestrator.as_str())
    );
    assert!(matches!(
        outcome.plan.planner,
        Some(PlannerRef::Tab { ref tab_id, ref session_id, .. })
            if tab_id == &planner && session_id.as_deref() == Some("planner-session-own")
    ));
}

#[test]
fn stale_foreign_or_unassigned_planner_cannot_create_plan() {
    let dir = tempfile::tempdir().unwrap();
    let harness = FirstPlanHarness::new(dir.path());
    let (_, _, caller) = bound_pair(&harness.mux, "valid");
    let mut stale = caller.clone();
    stale.binding_nonce = Some("stale".into());
    assert!(planner_create(&harness, stale, "Stale", 1).is_err());
    let mut foreign = caller.clone();
    foreign.session_id = Some("foreign".into());
    assert!(planner_create(&harness, foreign, "Foreign", 2).is_err());
    let unassigned = FirstPlanCaller {
        tab_id: Some(teamless_orchestrator(&harness.mux)),
        session_id: Some("none".into()),
        binding_nonce: Some("none".into()),
    };
    assert!(planner_create(&harness, unassigned, "Unassigned", 3).is_err());
    assert!(harness.plan_store.list_index().is_empty());
}

#[test]
fn planner_plan_creation_does_not_grant_execution_authority() {
    let dir = tempfile::tempdir().unwrap();
    let harness = FirstPlanHarness::new(dir.path());
    let (orchestrator, planner, caller) = bound_pair(&harness.mux, "authority");
    let plan = planner_create(&harness, caller, "Authority", 1)
        .unwrap()
        .plan;
    assert_eq!(plan.owner.tab_id.as_deref(), Some(orchestrator.as_str()));
    assert_ne!(plan.owner.tab_id.as_deref(), Some(planner.as_str()));
}

#[test]
fn concurrent_planner_plan_create_never_duplicates_team_plan() {
    let dir = tempfile::tempdir().unwrap();
    let harness = Arc::new(FirstPlanHarness::new(dir.path()));
    let (_, _, caller) = bound_pair(&harness.mux, "race");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|index| {
            let harness = Arc::clone(&harness);
            let caller = caller.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                planner_create(&harness, caller, &format!("Race {index}"), index + 1)
            })
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(FirstPlanTeamError::Plan(
                    PlanError::TeamAlreadyHasActivePlan { .. }
                ))
            ))
            .count(),
        1
    );
    assert_eq!(harness.plan_store.list_index().len(), 1);
}

#[test]
fn plan_create_uses_atomic_team_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let harness = FirstPlanHarness::new(dir.path());
    let owner_tab_id = teamless_orchestrator(&harness.mux);
    let spec = harness
        .plan_store
        .create_spec("Requirements".into(), dir.path().display().to_string(), 1)
        .unwrap();
    let outcome = create_first_plan_for_orchestrator(
        &harness.mux,
        &harness.plan_store,
        "Spec effort".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner_tab_id),
        vec![],
        Some(spec.id.clone()),
        2,
    )
    .unwrap();
    assert!(outcome.team_minted);
    assert_eq!(
        outcome.plan.primary_spec_id.as_deref(),
        Some(spec.id.as_str())
    );
    assert_eq!(
        outcome.plan.team_id.as_deref(),
        Some(outcome.team_id.as_str())
    );
}

#[test]
fn plan_goal_autocreate_uses_atomic_team_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let harness = FirstPlanHarness::new(dir.path());
    let owner_tab_id = teamless_orchestrator(&harness.mux);
    let outcome = create_first_plan_for_orchestrator(
        &harness.mux,
        &harness.plan_store,
        "Goal effort".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner_tab_id),
        vec![],
        None,
        1,
    )
    .unwrap();
    harness
        .plan_store
        .set_goal(
            &outcome.plan.id,
            Goal {
                vector: "Ship the effort".into(),
                ..Goal::default()
            },
            2,
        )
        .unwrap();
    assert_eq!(
        outcome.plan.team_id.as_deref(),
        Some(outcome.team_id.as_str())
    );
    assert_eq!(harness.mux.read().team_count(), 1);
}

#[test]
fn first_plan_response_has_team_naming_reminder() {
    let dir = tempfile::tempdir().unwrap();
    let harness = FirstPlanHarness::new(dir.path());
    let owner_tab_id = teamless_orchestrator(&harness.mux);
    let outcome = create_first_plan_for_orchestrator(
        &harness.mux,
        &harness.plan_store,
        "Named effort".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner_tab_id),
        vec![],
        None,
        1,
    )
    .unwrap();
    let mut response = serde_json::json!({ "plan": outcome.plan });
    outcome.apply_to_response(&mut response);
    let reminder = response["_hints"][0].as_str().unwrap();
    assert!(reminder.contains(&format!("codeterm team rename {} <name>", outcome.team_id)));
    assert!(reminder.contains("codeterm docs ORCHESTRATION --section durable-model"));
}

#[test]
fn existing_team_second_plan_preserves_hard_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let harness = FirstPlanHarness::new(dir.path());
    let owner_tab_id = teamless_orchestrator(&harness.mux);
    let first = create_first_plan_for_orchestrator(
        &harness.mux,
        &harness.plan_store,
        "First".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner_tab_id),
        vec![],
        None,
        1,
    )
    .unwrap();
    let error = create_first_plan_for_orchestrator(
        &harness.mux,
        &harness.plan_store,
        "Second".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner_tab_id),
        vec![],
        None,
        2,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        FirstPlanTeamError::Plan(PlanError::TeamAlreadyHasActivePlan {
            team_id,
            existing_plan_id,
            ..
        }) if team_id == first.team_id && existing_plan_id == first.plan.id
    ));
    assert_eq!(harness.plan_store.list_index().len(), 1);
}

#[test]
fn plan_entrypoints_use_shared_team_service_and_response() {
    let sources = [
        include_str!("../../../api/plan_routes/plans.rs"),
        include_str!("../../../socket/server/plan.rs"),
        include_str!("../../../socket/server/plan_goal.rs"),
    ];
    for source in sources {
        assert!(source.contains("create_first_plan_for_orchestrator"));
        assert!(source.contains("apply_to_response"));
        assert!(!source.contains(".create_plan("));
        assert!(!source.contains(".create_plan_from_spec("));
    }
}

#[test]
fn tauri_mock_app_handle_constructs_and_drops() {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let handle = app.handle().clone();
    drop(handle);
    drop(app);
}
