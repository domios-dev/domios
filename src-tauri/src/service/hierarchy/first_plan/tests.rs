use std::sync::Arc;

use crate::lock_hold::TimedRwLock;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{Goal, OrchestratorRef};
use crate::service::hierarchy::first_plan::create_first_plan_for_orchestrator;
use crate::service::plan::{PlanError, PlanStore};
use crate::service::FirstPlanTeamError;

struct FirstPlanHarness {
    mux: MuxState,
    plan_store: Arc<PlanStore>,
}

impl FirstPlanHarness {
    fn new(root: &std::path::Path) -> Self {
        let (state, _) = MultiplexerState::new();
        Self {
            mux: Arc::new(TimedRwLock::new("mux", state)),
            plan_store: Arc::new(PlanStore::new_for_test(root)),
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
    assert!(reminder.contains("codeterm docs ORCHESTRATION"));
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
