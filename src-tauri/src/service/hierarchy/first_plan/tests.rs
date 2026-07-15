use std::sync::atomic::Ordering;

use crate::multiplexer::types::AgentRole;
use crate::plan::{Goal, OrchestratorRef};
use crate::service::plan::PlanError;
use crate::service::test_support::test_harness;
use crate::service::{FirstPlanTeamError, ServiceState};

fn teamless_orchestrator(service: &ServiceState) -> String {
    let mut mux = service.mux.write();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let owner_pane_id = mux.create_tab(&workspace_id).unwrap().1;
    mux.set_pane_role(&owner_pane_id, AgentRole::Orchestrator, None)
        .unwrap();
    owner_pane_id
}

#[test]
fn plan_create_uses_atomic_team_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let harness = test_harness(dir.path(), "test-provider");
    harness
        .service
        .mux_emit_stats
        .frozen
        .store(true, Ordering::Release);
    let owner_pane_id = teamless_orchestrator(&harness.service);
    let spec = harness
        .service
        .plan_store
        .create_spec("Requirements".into(), dir.path().display().to_string(), 1)
        .unwrap();
    let outcome = harness
        .service
        .create_first_plan_for_orchestrator(
            "Spec effort".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane(&owner_pane_id),
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
    std::mem::forget(harness);
}

#[test]
fn plan_goal_autocreate_uses_atomic_team_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let harness = test_harness(dir.path(), "test-provider");
    harness
        .service
        .mux_emit_stats
        .frozen
        .store(true, Ordering::Release);
    let owner_pane_id = teamless_orchestrator(&harness.service);
    let outcome = harness
        .service
        .create_first_plan_for_orchestrator(
            "Goal effort".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane(&owner_pane_id),
            vec![],
            None,
            1,
        )
        .unwrap();
    harness
        .service
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
    assert_eq!(harness.service.mux.read().team_count(), 1);
    std::mem::forget(harness);
}

#[test]
fn first_plan_response_has_team_naming_reminder() {
    let dir = tempfile::tempdir().unwrap();
    let harness = test_harness(dir.path(), "test-provider");
    harness
        .service
        .mux_emit_stats
        .frozen
        .store(true, Ordering::Release);
    let owner_pane_id = teamless_orchestrator(&harness.service);
    let outcome = harness
        .service
        .create_first_plan_for_orchestrator(
            "Named effort".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane(&owner_pane_id),
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
    std::mem::forget(harness);
}

#[test]
fn existing_team_second_plan_preserves_hard_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let harness = test_harness(dir.path(), "test-provider");
    harness
        .service
        .mux_emit_stats
        .frozen
        .store(true, Ordering::Release);
    let owner_pane_id = teamless_orchestrator(&harness.service);
    let first = harness
        .service
        .create_first_plan_for_orchestrator(
            "First".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane(&owner_pane_id),
            vec![],
            None,
            1,
        )
        .unwrap();
    let error = harness
        .service
        .create_first_plan_for_orchestrator(
            "Second".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane(&owner_pane_id),
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
    assert_eq!(harness.service.plan_store.list_index().len(), 1);
    std::mem::forget(harness);
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
