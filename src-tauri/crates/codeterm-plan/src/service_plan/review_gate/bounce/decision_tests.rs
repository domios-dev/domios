use super::*;
use crate::plan::{Task, TaskExecutionAttempt, TaskExecutionReportSnapshot};

fn cleaned_task() -> Task {
    let mut task = crate::service_plan::test_support::task("review");
    task.status = TaskStatus::InReview;
    task.lane.cleanup_status = Some("done".into());
    for generation in 1..=2 {
        task.execution_attempts.push(TaskExecutionAttempt {
            id: format!("attempt-{generation}"),
            executor_tab_id: format!("worker-{generation}"),
            session_id: Some(format!("session-{generation}")),
            session_binding_nonce: Some(format!("nonce-{generation}")),
            assigned_at: Some(generation * 10),
            acknowledged_at: Some(generation * 10 + 1),
            reported_at: Some(generation * 10 + 2),
            closed_at: Some(generation * 10 + 3),
            retired_at: Some(generation * 10 + 3),
            report: Some(TaskExecutionReportSnapshot {
                id: format!("report-{generation}"),
                status: Some("in_review".into()),
                ..Default::default()
            }),
            ..Default::default()
        });
    }
    assert_eq!(cleaned_terminal_review_attempt(&task), Some(1));
    task
}

fn assert_refused(mut task: Task, change: impl FnOnce(&mut Task)) {
    change(&mut task);
    let before = task.clone();
    assert_eq!(cleaned_terminal_review_attempt(&task), None);
    assert_eq!(task, before);
}

#[test]
fn cleaned_review_model_refuses_owner_or_placement() {
    for case in 0..10 {
        assert_refused(cleaned_task(), |task| match case {
            0 => task.lane.tab_id = Some("remaining-tab".into()),
            1 => task.lane.worktree_path = Some("/remaining/worktree".into()),
            2 => task.lane.branch = Some("lane/remaining".into()),
            3 => task.lane.workspace_id = Some("workspace".into()),
            4 => task.lane.workspace_name = Some("workspace".into()),
            5 => task.lane.cleanup_status = Some("deferred".into()),
            6 => task.peer = Some("peer".into()),
            7 => task.assigned_tab_id = Some("owner".into()),
            8 => task.interruption.owner_tab_id = Some("owner".into()),
            _ => task.interruption.owner_session_id = Some("session".into()),
        });
    }
}

#[test]
fn cleaned_review_model_refuses_invalid_generation_order() {
    for case in 0..5 {
        assert_refused(cleaned_task(), |task| match case {
            0 => task.execution_attempts[1].assigned_at = task.execution_attempts[0].assigned_at,
            1 => task.execution_attempts[1].assigned_at = Some(9),
            2 => task.execution_attempts[1].id = task.execution_attempts[0].id.clone(),
            3 => {
                task.execution_attempts[1].session_id =
                    task.execution_attempts[0].session_id.clone()
            }
            _ => task.execution_attempts[0].closed_at = Some(21),
        });
    }
}

#[test]
fn cleaned_review_model_refuses_incomplete_terminal_report() {
    for case in 0..6 {
        assert_refused(cleaned_task(), |task| {
            let terminal = task.execution_attempts.last_mut().unwrap();
            match case {
                0 => {
                    terminal.report = None;
                    terminal.reported_at = None;
                }
                1 => terminal.acknowledged_at = None,
                2 => terminal.session_id = None,
                3 => terminal.report.as_mut().unwrap().status = Some("partial".into()),
                4 => terminal.retired_at = None,
                _ => {
                    terminal.report.as_mut().unwrap().reporter_tab_id =
                        Some("foreign-worker".into())
                }
            }
        });
    }
}
