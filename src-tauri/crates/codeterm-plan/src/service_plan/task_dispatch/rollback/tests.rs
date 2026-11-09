use super::*;
use crate::plan::{OrchestratorRef, Plan, Task, TaskLaneMetadata, TaskStatus};
use crate::service_plan::NewTask;

const STALE_TAB: &str = "tab-attempt-a";
const LIVE_TAB: &str = "tab-attempt-b";

fn pending_task() -> (tempfile::TempDir, PlanStore, String, Task) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "late attempt".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let original = store.inspect_task_for_test(&task_id).unwrap().task;
    (dir, store, plan.id, original)
}

fn task_of(store: &PlanStore, task_id: &str) -> Task {
    store.inspect_task_for_test(task_id).unwrap().task
}

fn begin(store: &PlanStore, plan_id: &str, task_id: &str, tab_id: &str, now: u64) -> String {
    store
        .begin_task_dispatch(
            plan_id,
            task_id,
            tab_id,
            "Ada",
            TaskLaneMetadata::default(),
            now,
        )
        .unwrap()
}

#[test]
fn late_attempt_failure_after_a_newer_live_attempt_keeps_the_task_on_it() {
    let (_dir, store, plan_id, original) = pending_task();
    let task_id = original.id.clone();
    let started = store
        .begin_task_dispatch_with_undo(
            &plan_id,
            &task_id,
            STALE_TAB,
            "Ada",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    let stale_attempt = started.attempt_id;
    let undo = started.undo;
    store
        .retire_stale_task_owner(&plan_id, &task_id, None, |_| false, 4)
        .unwrap();
    let live_attempt = begin(&store, &plan_id, &task_id, LIVE_TAB, 5);
    store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            LIVE_TAB,
            &live_attempt,
            "session-b",
            "nonce-b",
            TaskLaneMetadata::default(),
            6,
        )
        .unwrap();
    let before = task_of(&store, &task_id);

    assert!(store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            STALE_TAB,
            &stale_attempt,
            "session-a",
            "nonce-a",
            TaskLaneMetadata::default(),
            7,
        )
        .is_err());
    assert!(!store
        .mark_worker_startup_deadline(&plan_id, &task_id, STALE_TAB, &stale_attempt, true, 8)
        .unwrap());
    let rollback = store
        .roll_back_task_dispatch_attempt(&undo, STALE_TAB, &stale_attempt, 9)
        .unwrap();

    assert_eq!(
        rollback,
        DispatchRollback::Superseded {
            live_tab_id: Some(LIVE_TAB.into()),
            live_worktree_path: None,
        }
    );
    let after = task_of(&store, &task_id);
    assert_eq!(
        after, before,
        "a superseded failure must not touch the task"
    );
    assert_eq!(after.assigned_tab_id.as_deref(), Some(LIVE_TAB));
    assert_eq!(after.status_label.as_deref(), Some("awaiting_worker_ack"));
    let live = after
        .execution_attempts
        .iter()
        .filter(|attempt| attempt.closed_at.is_none())
        .collect::<Vec<_>>();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].id, live_attempt);
    let history = after
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == stale_attempt)
        .expect("the stale attempt stays in history");
    assert!(history.closed_at.is_some());
    assert!(history.retired_at.is_some());
    assert!(store
        .worker_startup_attempt_owns(&plan_id, &task_id, LIVE_TAB, &live_attempt, None)
        .unwrap());
}

#[test]
fn late_attempt_failure_replaced_without_retirement_is_still_fenced() {
    let (_dir, store, plan_id, original) = pending_task();
    let task_id = original.id.clone();
    let started = store
        .begin_task_dispatch_with_undo(
            &plan_id,
            &task_id,
            STALE_TAB,
            "Ada",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    let stale_attempt = started.attempt_id;
    let undo = started.undo;
    let live_attempt = begin(&store, &plan_id, &task_id, LIVE_TAB, 4);
    let before = task_of(&store, &task_id);

    let rollback = store
        .roll_back_task_dispatch_attempt(&undo, STALE_TAB, &stale_attempt, 5)
        .unwrap();

    assert!(matches!(rollback, DispatchRollback::Superseded { .. }));
    let after = task_of(&store, &task_id);
    assert_eq!(after, before);
    assert_eq!(
        after
            .execution_attempts
            .iter()
            .rfind(|attempt| attempt.closed_at.is_none())
            .map(|attempt| attempt.id.as_str()),
        Some(live_attempt.as_str())
    );
}

#[test]
fn late_attempt_failure_of_the_current_attempt_still_rolls_back() {
    let (_dir, store, plan_id, original) = pending_task();
    let task_id = original.id.clone();
    let started = store
        .begin_task_dispatch_with_undo(
            &plan_id,
            &task_id,
            STALE_TAB,
            "Ada",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    let attempt = started.attempt_id;
    let undo = started.undo;

    let rollback = store
        .roll_back_task_dispatch_attempt(&undo, STALE_TAB, &attempt, 4)
        .unwrap();

    assert_eq!(rollback, DispatchRollback::Restored);
    let restored = task_of(&store, &task_id);
    assert_eq!(restored.status, original.status);
    assert_eq!(restored.assigned_tab_id, original.assigned_tab_id);
    assert_eq!(restored.lane, original.lane);
    assert_eq!(restored.interruption, original.interruption);
    assert_eq!(restored.execution_attempts.len(), 1);
    assert_eq!(restored.execution_attempts[0].id, attempt);
    assert_eq!(restored.execution_attempts[0].closed_at, Some(4));
}

#[test]
fn final_startup_deadline_releases_the_worker_tab_assignment() {
    let (_dir, store, plan_id, original) = pending_task();
    let task_id = original.id.clone();
    let attempt = store
        .begin_task_dispatch(
            &plan_id,
            &task_id,
            STALE_TAB,
            "Ada",
            TaskLaneMetadata {
                tab_id: Some(STALE_TAB.into()),
                worktree_path: Some("/tmp/worker-lane".into()),
                branch: Some("lane/worker".into()),
                ..Default::default()
            },
            3,
        )
        .unwrap();

    assert!(store
        .mark_worker_startup_deadline(&plan_id, &task_id, STALE_TAB, &attempt, true, 4)
        .unwrap());

    let failed = task_of(&store, &task_id);
    assert!(failed.assigned_tab_id.is_none());
    assert!(failed.lane.tab_id.is_none());
    assert!(store.task_bindings_for_tab(STALE_TAB).unwrap().is_empty());
    assert_eq!(
        failed.lane.worktree_path.as_deref(),
        Some("/tmp/worker-lane")
    );
    assert_eq!(failed.lane.branch.as_deref(), Some("lane/worker"));
}

#[test]
fn late_attempt_failure_of_a_surviving_current_attempt_records_history() {
    let (_dir, store, plan_id, original) = pending_task();
    let task_id = original.id.clone();
    let attempt = begin(&store, &plan_id, &task_id, STALE_TAB, 3);

    assert!(store
        .mark_worker_startup_deadline(&plan_id, &task_id, STALE_TAB, &attempt, true, 4)
        .unwrap());

    let after = task_of(&store, &task_id);
    assert_eq!(after.status, TaskStatus::Pending);
    assert_eq!(after.status_label.as_deref(), Some("startup_failed"));
    assert!(after.assigned_tab_id.is_none());
    let history = after
        .execution_attempts
        .iter()
        .find(|candidate| candidate.id == attempt)
        .expect("the failed attempt stays in history");
    assert_eq!(history.closed_at, Some(4));
}

#[test]
fn dispatch_rollback_preserves_concurrent_task_edits_and_integration_metadata() {
    let (_dir, store, plan_id, original) = pending_task();
    let started = store
        .begin_task_dispatch_with_undo(
            &plan_id,
            &original.id,
            STALE_TAB,
            "Ada",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    store
        .edit_plan_fixture(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == original.id)
                .unwrap();
            task.name = "concurrent name".into();
            task.body = "concurrent body".into();
            task.files.edit.push("concurrent.rs".into());
            task.tags.push("concurrent-tag".into());
            task.acceptance_criteria.push("concurrent criterion".into());
            task.interruption.handoff_required = true;
            task.lane.destination = Some("integration-target".into());
            task.lane.cleanup_status = Some("integration-owned".into());
            task.lane.cleanup_hints.push("integration hint".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        store
            .roll_back_task_dispatch_attempt(&started.undo, STALE_TAB, &started.attempt_id, 5)
            .unwrap(),
        DispatchRollback::Restored
    );
    let task = task_of(&store, &original.id);
    assert_eq!(task.name, "concurrent name");
    assert_eq!(task.body, "concurrent body");
    assert_eq!(task.files.edit, ["concurrent.rs"]);
    assert_eq!(task.tags, ["concurrent-tag"]);
    assert_eq!(task.acceptance_criteria, ["concurrent criterion"]);
    assert!(task.interruption.handoff_required);
    assert_eq!(task.lane.destination.as_deref(), Some("integration-target"));
    assert_eq!(
        task.lane.cleanup_status.as_deref(),
        Some("integration-owned")
    );
    assert_eq!(task.lane.cleanup_hints, ["integration hint"]);
    assert_eq!(task.status, original.status);
    assert_eq!(task.assigned_tab_id, original.assigned_tab_id);
    assert_eq!(task.execution_attempts.last().unwrap().closed_at, Some(5));
    assert!(matches!(
        store
            .roll_back_task_dispatch_attempt(&started.undo, STALE_TAB, &started.attempt_id, 6)
            .unwrap(),
        DispatchRollback::Superseded { .. }
    ));
}

#[test]
fn dispatch_rollback_refuses_a_changed_owner_epoch_without_reverting_edits() {
    let (_dir, store, plan_id, original) = pending_task();
    let started = store
        .begin_task_dispatch_with_undo(
            &plan_id,
            &original.id,
            STALE_TAB,
            "Ada",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    store
        .edit_plan_fixture(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == original.id)
                .unwrap();
            task.interruption.owner_epoch += 1;
            task.body = "new owner".into();
            Ok(())
        })
        .unwrap();
    let before = task_of(&store, &original.id);
    assert!(matches!(
        store
            .roll_back_task_dispatch_attempt(&started.undo, STALE_TAB, &started.attempt_id, 5)
            .unwrap(),
        DispatchRollback::Superseded { .. }
    ));
    assert_eq!(task_of(&store, &original.id), before);
}

#[test]
fn dispatch_rollback_reverts_only_the_destination_written_by_startup() {
    for (before, concurrent) in [
        (None, None),
        (Some("existing-integration"), None),
        (None, Some("new-integration")),
    ] {
        let (_dir, store, plan_id, original) = pending_task();
        store
            .edit_plan_fixture(&plan_id, 3, |plan| {
                plan.tasks
                    .iter_mut()
                    .find(|task| task.id == original.id)
                    .unwrap()
                    .lane
                    .destination = before.map(str::to_string);
                Ok(())
            })
            .unwrap();
        let started = store
            .begin_task_dispatch_with_undo(
                &plan_id,
                &original.id,
                STALE_TAB,
                "Ada",
                TaskLaneMetadata {
                    destination: Some("startup-destination".into()),
                    ..Default::default()
                },
                4,
            )
            .unwrap();
        assert_eq!(
            task_of(&store, &original.id).lane.destination.as_deref(),
            before.or(Some("startup-destination"))
        );
        if let Some(destination) = concurrent {
            store
                .edit_plan_fixture(&plan_id, 5, |plan| {
                    plan.tasks
                        .iter_mut()
                        .find(|task| task.id == original.id)
                        .unwrap()
                        .lane
                        .destination = Some(destination.into());
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(
            store
                .roll_back_task_dispatch_attempt(&started.undo, STALE_TAB, &started.attempt_id, 6,)
                .unwrap(),
            DispatchRollback::Restored
        );
        let restored = task_of(&store, &original.id);
        assert_eq!(restored.lane.destination.as_deref(), concurrent.or(before));
        assert_eq!(
            restored
                .execution_attempts
                .last()
                .unwrap()
                .destination
                .as_deref(),
            concurrent.or(before)
        );
    }
}
