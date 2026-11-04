use super::*;
use codeterm_service_core::plan::{OrchestratorRef, TaskExecutionAttempt};

#[test]
fn selected_lane_inventory_and_refusals_match_recorded_task_and_historical_attempts() {
    for variant in 0..8 {
        let mut plan = Plan::new(
            "fixture".into(),
            "/canonical".into(),
            OrchestratorRef::default(),
        );
        let mut task: Task = serde_json::from_value(
            serde_json::json!({"id":"task","name":"fixture","type":"code","status":"done"}),
        )
        .unwrap();
        task.lane.worktree_path = Some("/lane".into());
        task.lane.branch = Some("lane/branch".into());
        task.lane.destination = Some("destination".into());
        task.assigned_tab_id = Some("assigned".into());
        task.execution_attempts.push(TaskExecutionAttempt {
            id: "attempt".into(),
            executor_tab_id: "executor".into(),
            worktree_path: Some("/historical".into()),
            branch: Some("lane/historical".into()),
            ..Default::default()
        });
        match variant {
            1 => task.status = TaskStatus::Pending,
            2 => task.execution_attempts[0].remote_worktree_path = Some("remote".into()),
            3 => task.lane.tab_id = Some("lane-tab".into()),
            4 => task.execution_attempts[0].destination = Some("conflict".into()),
            5 => task.execution_attempts[0].worktree_path = task.lane.worktree_path.clone(),
            6 => task.execution_attempts[0].branch = task.lane.branch.clone(),
            7 => plan.canonical_project_path = "/other-canonical".into(),
            _ => {}
        }
        plan.tasks.push(task);
        let facts = from_plan(&plan);
        let selected = cleanup_inventory(&facts);
        let original = super::super::cleanup_inventory(&plan);
        assert_eq!(selected, original);
        for live in [
            HashSet::new(),
            HashSet::from(["assigned".into()]),
            HashSet::from(["executor".into()]),
            HashSet::from(["lane-tab".into()]),
        ] {
            for lane in &selected {
                assert_eq!(
                    task_refusal(&facts.tasks[0], lane, &live),
                    super::super::task_refusal(&plan.tasks[0], lane, &live)
                );
            }
        }
    }
}

#[test]
fn lane_cleanup_propagates_snapshot_failure_before_attempting_removal() {
    let lane = TeamCloseLane {
        plan_id: "plan".into(),
        task_id: "task".into(),
        project_path: "/canonical".into(),
        worktree_path: Some("/lane".into()),
        branch: Some("lane/branch".into()),
        base: None,
        destination: None,
        hold_reason: None,
    };
    let result = run_lane_with_inventory(&lane, true, &Inventory::default(), || {
        Err("SQL failure-token".into())
    });
    assert_eq!(result.unwrap_err(), "SQL failure-token");
}
