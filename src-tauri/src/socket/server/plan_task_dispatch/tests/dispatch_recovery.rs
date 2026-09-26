use crate::multiplexer::state::MultiplexerState;
use crate::plan::{
    OrchestratorRef, PlanMode, PlanningState, TaskExecutionAttempt, TaskExecutionIntent,
};
use crate::service::plan::{NewTask, TaskLaneUpdate as LaneUpdate};
use crate::service::test_fixture::ServiceFixture;
use serde_json::json;
use std::sync::Arc;

use super::super::dispatch;

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn dispatch_runs_vanished_lane_placement_before_launch_failure() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let owner_tab = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&owner_tab).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&owner_tab)
        .unwrap()
        .team_id;
    {
        let tab = mux.get_tab_mut(&owner_tab).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.session_binding_nonce = Some("owner-nonce".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let project = fixture.temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    super::git(&project, &["init", "-q"]);
    super::git(&project, &["config", "user.email", "test@example.com"]);
    super::git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    super::git(&project, &["add", "README.md"]);
    super::git(&project, &["commit", "-qm", "fixture"]);

    let mut plan = fixture
        .service
        .plan_store
        .create_plan_for_team(
            "vanished lane dispatch".into(),
            project.display().to_string(),
            OrchestratorRef {
                tab_id: Some(owner_tab.clone()),
                session_id: Some("owner-session".into()),
                title: None,
            },
            team_id,
            vec![],
            1,
        )
        .unwrap();
    plan.mode = PlanMode::Approved;
    plan.planning_state = PlanningState::Approved;
    fixture.service.plan_store.save(&mut plan, 2).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "redispatch vanished lane".into(),
                kind: "implement".into(),
                execution_intent: TaskExecutionIntent::RepositoryMutation,
                ..Default::default()
            },
            3,
        )
        .unwrap();

    let old_lane = crate::service::provision_lane_for_test(
        project.to_str().unwrap(),
        &format!("{}-{task_id}", plan.id),
    )
    .unwrap();
    let old = old_lane.verify().unwrap();
    fixture
        .service
        .plan_store
        .update_task_lane(
            &plan.id,
            &task_id,
            LaneUpdate {
                tab_id: Some(Some("old-tab".into())),
                workspace_id: Some(Some("old-workspace".into())),
                workspace_name: Some(Some("old workspace".into())),
                worktree_path: Some(Some(old.worktree_path.clone())),
                branch: Some(Some(old.branch.clone())),
                base: Some(Some(old.base.clone())),
                destination: Some(Some(old.base.clone())),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .with_plan(&plan.id, 5, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap()
                .execution_attempts
                .push(TaskExecutionAttempt {
                    actual_reasoning_effort: None,
                    id: "old-attempt".into(),
                    executor_tab_id: "old-tab".into(),
                    branch: Some(old.branch.clone()),
                    worktree_path: Some(old.worktree_path.clone()),
                    ..Default::default()
                });
            Ok(())
        })
        .unwrap();
    std::fs::remove_dir_all(&old.worktree_path).unwrap();
    super::git(&project, &["worktree", "prune", "--expire", "now"]);
    super::git(&project, &["branch", "-D", &old.branch]);

    let provider_id = "vanished-lane-test-provider";
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_empty_launch_command_when_task_present(
            provider_id,
        )
        .with_launch_binary("cmd.exe"),
    ));
    fixture
        .service
        .require_agent_provider_ready(provider_id)
        .expect("the mock provider is available for dispatch");
    let service = Arc::new(fixture.service.clone());
    let params = Some(json!({
        "plan_id": plan.id.clone(),
        "task_id": task_id.clone(),
        "caller_tab_id": owner_tab.clone(),
        "parent_tab_id": owner_tab.clone(),
        "session_id": "owner-session",
        "session_binding_nonce": "owner-nonce",
        "execution_intent": "mutating",
        "agent": provider_id,
        "cwd": project.display().to_string(),
    }));

    let error = dispatch(&params, fixture._app.handle(), &service, 6)
        .expect_err("mock launch refusal occurs after lane placement and provisioning");

    assert_eq!(error.data.as_ref().unwrap()["stage"], "agent_spawn");
    let task = fixture.service.plan_store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, crate::plan::TaskStatus::Pending);
    assert_eq!(task.assigned_tab_id.as_deref(), Some("old-tab"));
    assert_eq!(
        task.lane.worktree_path.as_deref(),
        Some(old.worktree_path.as_str())
    );
    assert_eq!(task.lane.branch.as_deref(), Some(old.branch.as_str()));
    assert_eq!(task.execution_attempts.len(), 2);
    assert_eq!(task.execution_attempts[0].id, "old-attempt");
    assert_eq!(
        task.execution_attempts[0].worktree_path.as_deref(),
        Some(old.worktree_path.as_str())
    );
    assert!(task.execution_attempts[1].closed_at.is_some());
}
