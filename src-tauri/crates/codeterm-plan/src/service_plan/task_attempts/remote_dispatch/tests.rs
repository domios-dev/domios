use super::*;
use crate::plan::{OrchestratorRef, Plan, TaskExecutionAttempt};
use crate::service_plan::NewTask;

fn remote_attempt() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "Remote dispatch".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("origin-orchestrator"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Remote task".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["report reaches origin".into()],
                test_plan: Some("focused test".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = "attempt_remote".to_string();
    store
        .edit_plan_fixture(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.execution_attempts.push(TaskExecutionAttempt {
                id: attempt_id.clone(),
                executor_tab_id: "mesh-worker:attempt_remote".into(),
                mesh_peer: Some("peer-origin".into()),
                remote_dispatch_state: Some(RemoteDispatchState::PeerPreflight),
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    (root, store, plan.id, task_id, attempt_id)
}

#[test]
fn remote_dispatch_ack_is_authenticated_and_idempotent() {
    let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
    store
        .record_remote_dispatch_lane(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "mesh-worker:attempt_remote",
            "lane/remote-attempt",
            "D:/peer/repo-worktrees/lane",
            "base-sha",
            "codex",
            "worker-nonce",
            3,
        )
        .unwrap();
    assert_eq!(
        store
            .inspect_task_for_test(&task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .session_binding_nonce
            .as_deref(),
        Some("worker-nonce")
    );
    assert!(store
        .acknowledge_remote_dispatch(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "mesh-worker:attempt_remote",
            4,
        )
        .unwrap());
    assert!(!store
        .acknowledge_remote_dispatch(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "mesh-worker:attempt_remote",
            5,
        )
        .unwrap());
    let detail = store.inspect_task_for_test(&task_id).unwrap();
    let attempt = &detail.task.execution_attempts[0];
    assert_eq!(
        attempt.remote_dispatch_state,
        Some(RemoteDispatchState::Acknowledged)
    );
    assert_eq!(attempt.acknowledged_at, Some(4));
}

#[test]
fn remote_dispatch_peer_mismatch_cannot_change_attempt_state() {
    let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
    assert!(store
        .acknowledge_remote_dispatch(
            &plan_id,
            &task_id,
            &attempt_id,
            "spoofed-peer",
            "mesh-worker:attempt_remote",
            4,
        )
        .is_err());
    let detail = store.inspect_task_for_test(&task_id).unwrap();
    let attempt = &detail.task.execution_attempts[0];
    assert_eq!(
        attempt.remote_dispatch_state,
        Some(RemoteDispatchState::PeerPreflight)
    );
    assert_eq!(attempt.acknowledged_at, None);
}

#[test]
fn mesh_dispatch_records_origin_attempt_and_relays_ack_and_report() {
    let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
    store
        .record_remote_dispatch_lane(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            "lane/remote-attempt",
            "D:/peer/repo-worktrees/lane",
            "base-sha",
            "codex",
            "worker-nonce",
            4,
        )
        .unwrap();
    assert!(store
        .acknowledge_remote_dispatch(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            5,
        )
        .unwrap());
    let report = TaskExecutionReportSnapshot {
        id: "remote-report-1".into(),
        message: Some("STATUS: in_review\nCHANGES: remote lane".into()),
        status: Some("in_review".into()),
        sha: Some("lane-sha".into()),
        reporter_tab_id: Some("remote-tab-1".into()),
        ..Default::default()
    };
    assert!(store
        .ingest_remote_dispatch_report(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            report.clone(),
            6,
        )
        .unwrap());
    assert!(!store
        .ingest_remote_dispatch_report(&plan_id, &task_id, &attempt_id, "peer-origin", report, 7,)
        .unwrap());
    let detail = store.inspect_task_for_test(&task_id).unwrap();
    assert_eq!(detail.task.status, TaskStatus::InReview);
    let attempt = &detail.task.execution_attempts[0];
    assert_eq!(attempt.mesh_peer.as_deref(), Some("peer-origin"));
    assert_eq!(
        attempt.remote_worktree_path.as_deref(),
        Some("D:/peer/repo-worktrees/lane")
    );
    assert_eq!(attempt.worktree_path, None);
    assert_eq!(detail.task.lane.worktree_path, None);
    assert_eq!(detail.task.lane.destination, attempt.destination);
    assert_eq!(
        attempt.remote_dispatch_state,
        Some(RemoteDispatchState::Reported)
    );
    assert_eq!(attempt.reported_at, Some(6));
    assert_eq!(
        attempt.report.as_ref().unwrap().sha.as_deref(),
        Some("lane-sha")
    );
}

#[test]
fn mesh_worker_peer_offline_marks_attempt_explicitly() {
    let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
    store
        .set_remote_dispatch_state(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            RemoteDispatchState::PeerOffline,
            9,
        )
        .unwrap();
    let detail = store.inspect_task_for_test(&task_id).unwrap();
    assert_eq!(
        detail.task.execution_attempts[0].remote_dispatch_state,
        Some(RemoteDispatchState::PeerOffline)
    );
}

#[test]
fn remote_attempt_ack_deadline_marks_attempt_explicitly() {
    let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
    store
        .record_remote_dispatch_lane(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            "lane/remote-attempt",
            "D:/peer/repo-worktrees/lane",
            "base-sha",
            "codex",
            "worker-nonce",
            4,
        )
        .unwrap();

    assert!(store
        .mark_remote_ack_timeout(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            5,
        )
        .unwrap());
    assert!(!store
        .mark_remote_ack_timeout(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            6,
        )
        .unwrap());

    let task = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.status_label.as_deref(), Some("remote_ack_timeout"));
    assert!(task.assigned_tab_id.is_none());
    let attempt = &task.execution_attempts[0];
    assert_eq!(attempt.executor_tab_id, "remote-tab-1");
    assert_eq!(
        attempt.remote_dispatch_state,
        Some(RemoteDispatchState::RemoteAckTimeout)
    );
    assert_eq!(
        attempt.closure_reason.as_deref(),
        Some("remote_ack_timeout")
    );
    assert_eq!(attempt.closed_at, Some(5));
}

#[test]
fn remote_peer_offline_is_explicit_attempt_state() {
    let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
    store
        .record_remote_dispatch_lane(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            "lane/remote-attempt",
            "D:/peer/repo-worktrees/lane",
            "base-sha",
            "codex",
            "worker-nonce",
            4,
        )
        .unwrap();
    assert!(store
        .mark_remote_peer_offline_before_ack(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            5,
        )
        .unwrap());

    let task = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.status_label.as_deref(), Some("peer_offline"));
    assert!(task.assigned_tab_id.is_none());
    assert_eq!(
        task.execution_attempts[0].remote_dispatch_state,
        Some(RemoteDispatchState::PeerOffline)
    );
}

#[test]
fn mesh_worker_peer_offline_attempt_can_be_retried() {
    let (_root, store, plan_id, task_id, first_attempt) = remote_attempt();
    store
        .set_remote_dispatch_state(
            &plan_id,
            &task_id,
            &first_attempt,
            "peer-origin",
            RemoteDispatchState::PeerOffline,
            9,
        )
        .unwrap();
    let closed = store.inspect_task_for_test(&task_id).unwrap();
    assert!(closed.task.execution_attempts[0].closed_at.is_some());
    assert!(closed.task.assigned_tab_id.is_none());
    assert!(closed.task.execution_attempts[0].executor_tab_id.is_empty());

    store
        .begin_remote_dispatch_attempt(
            &plan_id,
            &task_id,
            "attempt_retry",
            "peer-origin",
            "peer",
            "codex",
            "base-sha",
            "main",
            10,
        )
        .unwrap();
    let retried = store.inspect_task_for_test(&task_id).unwrap();
    assert_eq!(retried.task.execution_attempts.len(), 2);
    assert_eq!(
        retried.task.assigned_tab_id.as_deref(),
        Some("mesh-worker:attempt_retry")
    );
}

#[test]
fn remote_lane_integration_requires_origin_review_and_records_result() {
    let (_root, store, plan_id, task_id, attempt_id) = remote_attempt();
    store
        .record_remote_dispatch_lane(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            "lane/remote-attempt",
            "D:/peer/repo-worktrees/lane",
            "base-sha",
            "codex",
            "worker-nonce",
            4,
        )
        .unwrap();
    store
        .acknowledge_remote_dispatch(
            &plan_id,
            &task_id,
            &attempt_id,
            "peer-origin",
            "remote-tab-1",
            5,
        )
        .unwrap();
    let report = TaskExecutionReportSnapshot {
        id: "remote-report-merge".into(),
        reporter_tab_id: Some("remote-tab-1".into()),
        ..Default::default()
    };
    store
        .ingest_remote_dispatch_report(&plan_id, &task_id, &attempt_id, "peer-origin", report, 6)
        .unwrap();
    assert!(store
        .mark_remote_dispatch_integrated(
            &plan_id,
            &task_id,
            &attempt_id,
            "lane-tip",
            "merge-tip",
            7,
        )
        .is_err());
    store
        .accept_review(
            &plan_id,
            &task_id,
            "origin review accepts the peer lane",
            false,
            false,
            None,
            crate::plan::AgentRef::from_tab("origin-orchestrator"),
            8,
        )
        .unwrap();
    assert_eq!(
        store
            .inspect_task_for_test(&task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .judge_verdict
            .as_deref(),
        Some("accepted")
    );
    store
        .mark_remote_dispatch_integrated(
            &plan_id,
            &task_id,
            &attempt_id,
            "lane-tip",
            "merge-tip",
            9,
        )
        .unwrap();
    store
        .mark_remote_dispatch_integrated(
            &plan_id,
            &task_id,
            &attempt_id,
            "lane-tip",
            "merge-tip",
            10,
        )
        .unwrap();
    let attempt = &store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task
        .execution_attempts[0];
    assert_eq!(
        attempt.remote_dispatch_state,
        Some(RemoteDispatchState::Integrated)
    );
    assert_eq!(attempt.remote_lane_tip.as_deref(), Some("lane-tip"));
    assert_eq!(attempt.remote_merge_commit.as_deref(), Some("merge-tip"));
}
