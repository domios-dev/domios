use super::*;

fn remote_dispatch_attempt(
    store: &PlanStore,
    project: &str,
    suffix: &str,
) -> (String, String, String) {
    let mut plan = Plan::new(
        format!("Remote {suffix}"),
        project.to_string(),
        OrchestratorRef::from_tab("origin-orchestrator"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: format!("Remote {suffix}"),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = format!("attempt-{suffix}");
    store
        .begin_remote_dispatch_attempt(
            &plan.id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "Origin peer",
            "codex",
            "base-sha",
            "main",
            3,
        )
        .unwrap();
    store
        .record_remote_dispatch_lane(
            &plan.id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-worker-tab",
            "lane/remote-worker",
            "D:/peer/lane",
            "base-sha",
            "codex",
            4,
        )
        .unwrap();
    (plan.id, task_id, attempt_id)
}

#[test]
fn remote_attempt_ack_deadline_marks_attempt_explicitly() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id, attempt_id) = remote_dispatch_attempt(&store, &project, "timeout");

    assert!(store
        .mark_remote_ack_timeout(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-worker-tab",
            5,
        )
        .unwrap());
    assert!(!store
        .mark_remote_ack_timeout(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-worker-tab",
            6,
        )
        .unwrap());

    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.assigned_tab_id.is_none());
    let attempt = &task.execution_attempts[0];
    assert_eq!(
        attempt.remote_dispatch_state,
        Some(crate::plan::RemoteDispatchState::RemoteAckTimeout)
    );
    assert_eq!(attempt.mesh_peer.as_deref(), Some("peer-origin"));
    assert_eq!(attempt.executor_tab_id, "remote-worker-tab");
}

#[test]
fn remote_peer_offline_is_explicit_attempt_state() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id, attempt_id) = remote_dispatch_attempt(&store, &project, "offline");

    assert!(store
        .mark_remote_peer_offline_before_ack(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-worker-tab",
            5,
        )
        .unwrap());
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.assigned_tab_id.is_none());
    assert_eq!(
        task.execution_attempts[0].remote_dispatch_state,
        Some(crate::plan::RemoteDispatchState::PeerOffline)
    );
}
