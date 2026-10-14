use super::*;
use crate::plan::{OrchestratorRef, Plan, PlannerRef, Task, TaskExecutionAttempt};

#[test]
fn plan_restore_recovers_pairing_without_reviving_finalized_tasks_or_lanes() {
    let mut saved = Plan::new(
        "Opaque plan".into(),
        "/project".into(),
        OrchestratorRef {
            tab_id: Some("orchestrator".into()),
            session_id: Some("own-orchestrator".into()),
            title: None,
        },
    );
    saved.team_id = Some("team".into());
    saved.planner = Some(PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: Some("own-planner".into()),
        title: None,
    });
    let mut task: Task = serde_json::from_value(
        serde_json::json!({"id": "task", "name": "Task", "type": "implement", "status": "done"}),
    )
    .unwrap();
    task.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt".into(),
        executor_tab_id: "worker".into(),
        closed_at: Some(200),
        ..Default::default()
    });
    let mut current = saved.clone();
    current.tasks.push(task);
    current.team_id = None;
    current.owner = Default::default();
    current.planner = None;
    let finalized_tasks = current.tasks.clone();
    restore_plan_metadata(&mut current, &saved);
    assert_eq!(current.team_id, saved.team_id);
    assert_eq!(current.owner, saved.owner);
    assert_eq!(current.planner, saved.planner);
    assert_eq!(current.tasks, finalized_tasks);
    let once = current.clone();
    restore_plan_metadata(&mut current, &saved);
    assert_eq!(current, once);
}

#[test]
fn automation_inventory_round_trip_preserves_enabled_and_paused_states() {
    let cron: crate::cron::store::CronJob = serde_json::from_value(serde_json::json!({
        "id": "cron", "name": "Cron", "workspace_id": "workspace", "team_id": "team", "schedule": "0 * * * * *",
        "enabled": false, "agent": "fixture-provider", "task": "opaque-task", "created_at": 10
    })).unwrap();
    let blueprint: crate::sentinel::store::Sentinel = serde_json::from_value(serde_json::json!({
        "id": "blueprint", "creator_tab_id": "orchestrator", "workspace_id": "workspace", "team_id": "team",
        "cwd": "/project", "probe": "opaque-probe", "question": "opaque-question", "buckets": ["ready"],
        "interval_ms": 1000, "routes": {}, "matches": {}, "next_run": 100,
        "machine": {"last_hash": "opaque-hash", "last_bucket": "ready", "paused": true, "stopped": false}
    })).unwrap();
    let original = Resources {
        plans: vec![],
        crons: vec![cron],
        blueprints: vec![serde_json::to_value(&blueprint).unwrap()],
    };
    let json = serde_json::to_value(&original).unwrap();
    let restored: Resources = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), json);
    assert!(!restored.crons[0].enabled);
    let restored_blueprint: crate::sentinel::store::Sentinel =
        serde_json::from_value(restored.blueprints[0].clone()).unwrap();
    assert!(restored_blueprint.machine.paused);
    assert!(!restored_blueprint.machine.stopped);
}
