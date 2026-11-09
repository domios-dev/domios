use super::*;
use crate::plan::TaskExecutionAttempt;

#[test]
fn remote_retirement_releases_assignment_and_fences_late_ack() {
    let mut task: Task = serde_json::from_value(serde_json::json!({
        "id": "task", "name": "Remote", "type": "implement", "status": TaskStatus::Pending
    }))
    .unwrap();
    task.status = TaskStatus::InProgress;
    task.assigned_tab_id = Some("worker".into());
    task.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt".into(),
        mesh_peer: Some("peer".into()),
        executor_tab_id: "worker".into(),
        branch: Some("lane/worker".into()),
        remote_worktree_path: Some("remote/lane".into()),
        ..Default::default()
    });
    assert!(!retire_remote_attempt(
        &mut task,
        "attempt",
        "foreign",
        "worker",
        "remote_tab_closed",
        7
    ));
    assert!(retire_remote_attempt(
        &mut task,
        "attempt",
        "peer",
        "worker",
        "remote_tab_closed",
        7
    ));
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.assigned_tab_id.is_none());
    assert_eq!(task.execution_attempts[0].closed_at, Some(7));
    assert_eq!(task.execution_attempts[0].retired_at, Some(7));
    assert_eq!(
        task.execution_attempts[0].branch.as_deref(),
        Some("lane/worker")
    );
    assert_eq!(
        task.execution_attempts[0].remote_worktree_path.as_deref(),
        Some("remote/lane")
    );
    assert!(!retire_remote_attempt(
        &mut task,
        "attempt",
        "peer",
        "worker",
        "remote_tab_closed",
        8
    ));
}
