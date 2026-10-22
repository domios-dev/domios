use std::path::Path;

use super::*;
use crate::plan::{AgentRef, OrchestratorRef, TaskStatus};
use crate::service_plan::{NewTask, PlanStore};

fn store_and_root() -> (tempfile::TempDir, PlanStore) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    (root, store)
}

#[test]
fn index_queue_keeps_only_the_latest_snapshot_per_document() {
    let queue = IndexQueue {
        state: Mutex::new(IndexQueueState::default()),
        ready: Condvar::new(),
    };
    let mut first = crate::plan::Plan::new(
        "First title".into(),
        "project".into(),
        OrchestratorRef::default(),
    );
    first.id = "pl_queue".into();
    let mut latest = first.clone();
    latest.title = "Latest title".into();

    queue.enqueue(IndexJob::Plan(first, PlanIndexScope::Full));
    queue.enqueue(IndexJob::Plan(latest, PlanIndexScope::Full));

    let state = queue.state.lock();
    assert_eq!(state.order.len(), 1);
    assert_eq!(state.latest.len(), 1);
    let IndexJob::Plan(snapshot, _) = state
        .latest
        .get(&format!(
            "{}:plan:pl_queue",
            project_scope(Path::new("project"))
        ))
        .unwrap()
    else {
        panic!("expected a plan snapshot");
    };
    assert_eq!(snapshot.title, "Latest title");
}

#[test]
fn close_task_immediate_readback_is_consistent() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Close readback".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Close me".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store.start_task_for_test(&plan.id, &task_id, 2);
    store
        .close_task(
            &plan.id,
            &task_id,
            Some("verified close"),
            AgentRef::from_tab("worker"),
            3,
        )
        .unwrap();

    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.status, TaskStatus::Done);
    assert!(task
        .milestones
        .iter()
        .any(|milestone| milestone.text == "verified close"));
}

#[test]
fn assign_task_readback_sets_tab_lane_and_peer() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Assign readback".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Assign me".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .assign_task(&plan.id, &task_id, "tab_2", Some("mac"), 3)
        .unwrap();

    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.assigned_tab_id.as_deref(), Some("tab_2"));
    assert_eq!(task.lane.tab_id.as_deref(), Some("tab_2"));
    assert_eq!(task.peer.as_deref(), Some("mac"));
}

#[test]
fn task_drop_readback_clears_assignment_role_and_peer() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Drop assignment".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Drop me".into(),
                kind: "fix".into(),
                assigned_tab_id: Some("tab_drop".into()),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "tab_drop", Some("mac"), 3)
        .unwrap();

    store.set_task_assignment(&task_id, None, None, 4).unwrap();

    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.assigned_tab_id, None);
    assert_eq!(task.taken_by_role, None);
    assert_eq!(task.peer, None);
}

#[path = "readback_lock_tests.rs"]
mod readback_lock_tests;

#[path = "tests/deletion.rs"]
mod deletion;

#[path = "decision_tests.rs"]
pub(super) mod decisions;
