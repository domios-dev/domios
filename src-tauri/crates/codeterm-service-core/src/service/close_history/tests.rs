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
    let blueprint: codeterm_sentinel::store::Sentinel = serde_json::from_value(serde_json::json!({
        "id": "blueprint", "creator_tab_id": "orchestrator", "workspace_id": "workspace", "team_id": "team",
        "cwd": "/project", "probe": "opaque-probe", "question": "opaque-question", "buckets": ["ready"],
        "interval_ms": 1000, "routes": {}, "matches": {}, "next_run": 100,
        "machine": {"last_hash": "opaque-hash", "last_bucket": "ready", "paused": true, "stopped": false}
    })).unwrap();
    let original = Resources {
        plans: vec![],
        crons: vec![cron],
        blueprints: vec![serde_json::to_value(&blueprint).unwrap()],
        ..Default::default()
    };
    let json = serde_json::to_value(&original).unwrap();
    let restored: Resources = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), json);
    assert!(!restored.crons[0].enabled);
    let restored_blueprint: codeterm_sentinel::store::Sentinel =
        serde_json::from_value(restored.blueprints[0].clone()).unwrap();
    assert!(restored_blueprint.machine.paused);
    assert!(!restored_blueprint.machine.stopped);
}

#[test]
fn restore_refuses_a_corrupt_saved_blueprint_before_any_effect() {
    let mut corrupt = Resources {
        blueprints: vec![serde_json::json!({"id": "b", "workspace_id": "w", "team_id": null})],
        ..Default::default()
    };
    assert!(validate_saved_blueprints(&mut corrupt).is_err());
    let mut unscoped = Resources {
        blueprints: vec![serde_json::json!({"cwd": "/project"})],
        ..Default::default()
    };
    assert!(validate_saved_blueprints(&mut unscoped).is_err());
    assert!(validate_saved_blueprints(&mut Resources::default()).is_ok());
}

fn native_saved_blueprint() -> serde_json::Value {
    let document = serde_json::json!({"schema_version":2,"kind":"pipeline","name":"restore-fixture","owner":"team","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"read","run":{"lit":"echo ready"}}]});
    let doc =
        codeterm_blueprint_types::blueprint_document::Document::from_definition(&document).unwrap();
    let runtime = doc.lower().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let store = codeterm_sentinel::store::Store::new(
        temp.path().join("fixture.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let mut job = store
        .prepare_pipeline(
            serde_json::from_value(runtime["spec"].clone()).unwrap(),
            codeterm_sentinel::store::Owner {
                creator_tab_id: "creator".into(),
                workspace_id: "workspace".into(),
                team_id: None,
                cwd: temp.path().to_string_lossy().into(),
            },
            true,
        )
        .unwrap();
    job.document = Some(document);
    serde_json::to_value(job).unwrap()
}

#[test]
fn blueprint_v2_check_restore_preflights_mixed_v1_and_v2_snapshot() {
    let native = native_saved_blueprint();
    let mut obsolete = native.clone();
    obsolete.as_object_mut().unwrap().remove("document");
    obsolete["id"] = serde_json::json!("obsolete");
    let mut resources = Resources {
        blueprints: vec![obsolete, native.clone()],
        ..Default::default()
    };
    validate_saved_blueprints(&mut resources).unwrap();
    assert_eq!(resources.blueprints, vec![native]);
    let receipt = serde_json::to_value(&resources).unwrap();
    assert_eq!(receipt["discarded_v1_blueprints"][0]["id"], "obsolete");
}

#[test]
fn blueprint_v2_check_restore_refuses_bad_v2_before_changes() {
    let mut invalid = native_saved_blueprint();
    invalid["document"]["steps"][0]["routes"] = serde_json::json!({"done":"read"});
    let mut resources = Resources {
        blueprints: vec![native_saved_blueprint(), invalid],
        ..Default::default()
    };
    let before = serde_json::to_value(&resources).unwrap();
    assert!(validate_saved_blueprints(&mut resources).is_err());
    assert_eq!(serde_json::to_value(&resources).unwrap(), before);
}

mod restore;
