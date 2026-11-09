use super::*;
use crate::service_plan::{WorkerLossRead, WorkerLossRecoveryRead, WorkerLossReplacementRead};

fn reference(plan: &Plan, task: &crate::plan::Task, tab: &str) -> WorkerLossRead {
    let own = task
        .execution_attempts
        .iter()
        .rfind(|attempt| attempt.executor_tab_id == tab);
    let replacement = own
        .and_then(|source| {
            let boundary = task
                .execution_attempts
                .iter()
                .rposition(|attempt| attempt.id == source.id)?;
            task.execution_attempts[boundary + 1..]
                .iter()
                .rev()
                .find(|attempt| attempt.closed_at.is_none())
        })
        .map(|attempt| WorkerLossReplacementRead {
            attempt_id: attempt.id.clone(),
            executor_tab_id: attempt.executor_tab_id.clone(),
        });
    WorkerLossRead {
        plan_id: plan.id.clone(),
        task_id: task.id.clone(),
        task_name: task.name.clone(),
        orchestrator_tab_id: plan.owner.tab_id.clone().filter(|owner| owner != tab),
        attempt_id: own.map(|attempt| attempt.id.clone()),
        worktree_path: own
            .and_then(|attempt| attempt.worktree_path.clone())
            .or_else(|| task.lane.worktree_path.clone()),
        branch: own
            .and_then(|attempt| attempt.branch.clone())
            .or_else(|| task.lane.branch.clone()),
        latest_attempt_id: task
            .execution_attempts
            .last()
            .map(|attempt| attempt.id.clone()),
        replacement,
    }
}

fn bound(task: &crate::plan::Task, tab: &str) -> bool {
    task.assigned_tab_id.as_deref() == Some(tab)
        || task.lane.tab_id.as_deref() == Some(tab)
        || task
            .execution_attempts
            .iter()
            .any(|attempt| attempt.executor_tab_id == tab && attempt.closed_at.is_none())
}

#[test]
fn worker_loss_projection_preserves_binding_source_duplicate_boundary_and_replacement() {
    for variant in 0..16 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        plan.tasks[0].status = TaskStatus::InProgress;
        plan.tasks[0].lane.worktree_path = Some("lane".into());
        plan.tasks[0].lane.branch = Some("lane-branch".into());
        match variant {
            1 => plan.status = PlanStatus::Archived,
            2 => plan.tasks[0].status = TaskStatus::Pending,
            3 => {
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].execution_attempts[0].retired_at = Some(1);
            }
            4 => {
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].execution_attempts[0].closed_at = Some(1);
            }
            5 => {
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].execution_attempts.clear();
                plan.tasks[0].lane.tab_id = Some("worker".into());
            }
            6 => {
                plan.tasks[0].execution_attempts[0].worktree_path = Some("attempt-lane".into());
                plan.tasks[0].execution_attempts[0].branch = Some("attempt-branch".into());
            }
            7 => {
                plan.tasks[0].execution_attempts[0].worktree_path = Some(String::new());
                plan.tasks[0].execution_attempts[0].branch = Some("\u{2003}".into());
            }
            8 => plan.owner.tab_id = Some("worker".into()),
            9..=13 => {
                let mut later = plan.tasks[0].execution_attempts[0].clone();
                later.id = "later".into();
                later.executor_tab_id = "replacement".into();
                match variant {
                    10 => later.retired_at = Some(1),
                    11 => later.closed_at = Some(1),
                    12 => later.id = "current".into(),
                    13 => later.executor_tab_id = "worker".into(),
                    _ => {}
                }
                plan.tasks[0].execution_attempts.push(later);
            }
            14 => plan.tasks[0].execution_attempts.clear(),
            15 => {
                plan.tasks[0].execution_attempts[0].executor_tab_id = "other".into();
                plan.tasks[0].assigned_tab_id = None;
                plan.tasks[0].lane.tab_id = None;
            }
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        let read = database
            .worker_loss_projection(Some(&plan.id), Some("task_exact"), "worker", false)
            .unwrap();
        assert_eq!(
            read,
            vec![reference(&plan, &plan.tasks[0], "worker")],
            "variant {variant}"
        );
        let list = database
            .worker_loss_projection(None, None, "worker", true)
            .unwrap();
        let expected = if plan.status == PlanStatus::Active
            && plan.tasks[0].status == TaskStatus::InProgress
            && bound(&plan.tasks[0], "worker")
        {
            vec![reference(&plan, &plan.tasks[0], "worker")]
        } else {
            vec![]
        };
        assert_eq!(list, expected, "variant {variant}");
        let recovery = database
            .worker_loss_recovery_projection(&plan.id, "task_exact", "worker")
            .unwrap()
            .unwrap();
        let expected = reference(&plan, &plan.tasks[0], "worker");
        assert_eq!(
            recovery,
            WorkerLossRecoveryRead {
                latest_attempt_id: expected.latest_attempt_id,
                replacement: expected.replacement
            }
        );
        assert!(database
            .worker_loss_recovery_projection(&plan.id, "missing", "worker")
            .unwrap()
            .is_none());
    }
}

#[test]
fn worker_loss_projection_observes_committed_replacement_and_skips_unrequested_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    assert!(database
        .worker_loss_recovery_projection(&plan.id, "task_exact", "worker")
        .unwrap()
        .unwrap()
        .replacement
        .is_none());
    let mut replacement = plan.tasks[0].execution_attempts[0].clone();
    replacement.id = "replacement".into();
    replacement.executor_tab_id = "new-worker".into();
    plan.tasks[0].execution_attempts.push(replacement);
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .worker_loss_recovery_projection(&plan.id, "task_exact", "worker")
            .unwrap()
            .unwrap()
            .replacement
            .unwrap()
            .executor_tab_id,
        "new-worker"
    );
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',42,'$.lane.worktree_path',42) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .worker_loss_projection(Some(&plan.id), Some("task_exact"), "worker", false)
        .is_err());
    assert!(database
        .worker_loss_recovery_projection(&plan.id, "task_exact", "worker")
        .is_ok());
}

#[test]
fn worker_loss_fixture_returns_only_selected_loss_and_recovery_facts_without_writes() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].status = TaskStatus::InProgress;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        let original = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| {
                let mut old = original.clone();
                old.id = format!("closed-{index}");
                old.closed_at = Some(2);
                old
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let loss = database
            .worker_loss_projection(None, None, "worker", true)
            .unwrap();
        let recovery = database
            .worker_loss_recovery_projection(&plan.id, "task_0000", "worker")
            .unwrap()
            .unwrap();
        assert_eq!(loss.len(), 1);
        assert_eq!(loss[0].attempt_id.as_deref(), Some("current"));
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(loss, recovery)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=worker_loss tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
