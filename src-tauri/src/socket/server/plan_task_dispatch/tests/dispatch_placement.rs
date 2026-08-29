use crate::plan::{ExecutionIntent, OrchestratorRef, TaskLaneMetadata};
use crate::service::plan::{NewTask, PlanStore, TaskLaneUpdate};

use super::super::{
    complete_dispatch_transaction, dispatch_ready_event, lane_resolution::DispatchLaneResolution,
    should_provision_worker_lane, DispatchEvidence,
};

const WORKER_TAB: &str = "worker-tab";

fn store_with_starting_dispatch() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "placement".into(),
            root.path().display().to_string(),
            OrchestratorRef::from_tab("orchestrator"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = store
        .begin_task_dispatch_with_policy(
            &plan.id,
            &task_id,
            WORKER_TAB,
            "Worker",
            TaskLaneMetadata::default(),
            ExecutionIntent::ReadOnly,
            None,
            Some("codex"),
            3,
        )
        .unwrap();
    (root, store, plan.id, task_id, attempt_id)
}

#[test]
fn dispatch_placement_read_only_keeps_canonical_metadata_empty() {
    let (_root, store, plan, task, attempt) = store_with_starting_dispatch();
    store
        .finish_task_dispatch(
            &plan,
            &task,
            WORKER_TAB,
            &attempt,
            "session",
            "nonce",
            TaskLaneMetadata::default(),
            4,
        )
        .unwrap();
    let task = store.show_task(&task).unwrap().task;
    assert!(task.lane.worktree_path.is_none());
}

#[test]
fn dispatch_placement_mutating_still_requires_an_isolated_lane() {
    let unresolved_lane = DispatchLaneResolution {
        spawn_cwd: "canonical".into(),
        existing: None,
    };
    assert!(should_provision_worker_lane(
        true,
        &ExecutionIntent::Mutating,
        Some(&unresolved_lane)
    ));
    assert!(!should_provision_worker_lane(
        true,
        &ExecutionIntent::ReadOnly,
        Some(&unresolved_lane)
    ));
    assert!(!should_provision_worker_lane(
        true,
        &ExecutionIntent::Mutating,
        None
    ));
}

#[test]
fn dispatch_placement_destination_update_preserves_the_starting_owner() {
    let (_root, store, plan, task, attempt) = store_with_starting_dispatch();
    store
        .update_task_lane(
            &plan,
            &task,
            TaskLaneUpdate {
                destination: Some(Some("release/1.10.1".into())),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let completed = complete_dispatch_transaction(
        std::time::Instant::now() + std::time::Duration::from_secs(1),
        |_| Ok(()),
        |_| Ok("session".into()),
        || Ok(None),
        |_, session| {
            store
                .finish_task_dispatch(
                    &plan,
                    &task,
                    WORKER_TAB,
                    &attempt,
                    session,
                    "nonce",
                    TaskLaneMetadata::default(),
                    5,
                )
                .map_err(|error| error.to_string())
        },
        || Ok(()),
    );
    assert!(
        completed.is_ok(),
        "destination update must not abort dispatch readiness"
    );
    assert_eq!(
        store
            .show_task(&task)
            .unwrap()
            .task
            .lane
            .destination
            .as_deref(),
        Some("release/1.10.1")
    );
}

#[test]
fn dispatch_placement_owner_replacement_still_aborts_readiness() {
    let (_root, store, plan, task, attempt) = store_with_starting_dispatch();
    store
        .update_task_lane(
            &plan,
            &task,
            TaskLaneUpdate {
                tab_id: Some(Some("replacement-tab".into())),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let result = store.finish_task_dispatch(
        &plan,
        &task,
        WORKER_TAB,
        &attempt,
        "session",
        "nonce",
        TaskLaneMetadata::default(),
        5,
    );
    assert!(
        result.is_err(),
        "a replacement owner must still fence the stale dispatch"
    );
}

#[test]
fn dispatch_placement_destination_advice_is_carried_by_dispatch_ready() {
    let event = dispatch_ready_event(
        "plan",
        "task",
        WORKER_TAB,
        "attempt",
        &DispatchEvidence {
            session_id: "session".into(),
            lane: None,
        },
        &super::super::lane_safety::dispatch_safety_nudges(
            "plan",
            "task",
            &TaskLaneMetadata {
                worktree_path: Some("lane".into()),
                ..Default::default()
            },
        ),
    );
    assert_eq!(event["kind"], "dispatch_ready");
    assert!(event["safety_nudges"]
        .as_array()
        .is_some_and(|nudges| !nudges.is_empty()));
}
