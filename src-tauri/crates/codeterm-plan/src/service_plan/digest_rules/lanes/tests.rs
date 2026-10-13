use super::*;
use crate::plan::{TaskExecutionAttempt, TaskStatus};
use crate::service_plan::digest_rules::tests::{minimal_task, plan_with_counts};

#[test]
fn counts_terminal_lane_paths_once_and_honors_cleanup_receipts() {
    let mut plan = plan_with_counts(0, 0);
    for (status, path, cleanup) in [
        (TaskStatus::Done, "/done", None),
        (TaskStatus::Cancelled, "/cancelled", Some("cleanup_debt")),
        (TaskStatus::InReview, "/review", None),
        (TaskStatus::InProgress, "/active", None),
        (TaskStatus::Done, "/cleaned", Some("done")),
        (TaskStatus::Cancelled, "/released", Some("released")),
        (TaskStatus::Done, " ", None),
    ] {
        let mut task = minimal_task(path, path, status, 0);
        task.lane.worktree_path = Some(path.into());
        task.lane.cleanup_status = cleanup.map(str::to_string);
        task.execution_attempts.push(TaskExecutionAttempt {
            worktree_path: Some(path.into()),
            ..Default::default()
        });
        plan.tasks.push(task);
    }
    let mut attempts_only = minimal_task("attempt", "attempt", TaskStatus::Done, 0);
    attempts_only.execution_attempts.push(TaskExecutionAttempt {
        worktree_path: Some("/attempt".into()),
        ..Default::default()
    });
    attempts_only.execution_attempts.push(TaskExecutionAttempt {
        remote_worktree_path: Some("/attempt".into()),
        mesh_peer: Some("peer".into()),
        ..Default::default()
    });
    plan.tasks.push(attempts_only);
    let (count, names) = recorded_finished_lanes(&plan);
    assert_eq!(count, 4);
    assert_eq!(names.len(), 4);
    assert!(names.iter().any(|name| name.starts_with("peer:/attempt")));
}

#[test]
fn candidate_list_is_bounded_and_prioritizes_recorded_dirty_or_unmerged_lanes() {
    let mut plan = plan_with_counts(0, 0);
    for index in 0..11 {
        let mut task = minimal_task(&format!("task-{index}"), "task", TaskStatus::Done, 0);
        task.lane.worktree_path = Some(format!("/lane-{index}"));
        if index == 9 {
            task.lane.cleanup_hints = vec!["dirty worktree".into()];
        }
        if index == 10 {
            task.lane.cleanup_hints = vec!["unmerged branch".into()];
        }
        plan.tasks.push(task);
    }
    let counters = crate::service_plan::digest_rules::plan_digest_counters(&plan, 0, 0);
    assert_eq!(counters.finished_lanes, 11);
    assert_eq!(counters.lane_candidates.len(), 5);
    assert!(counters
        .lane_candidates
        .iter()
        .any(|name| name.starts_with("/lane-9 ")));
    assert!(counters
        .lane_candidates
        .iter()
        .any(|name| name.starts_with("/lane-10 ")));
}
