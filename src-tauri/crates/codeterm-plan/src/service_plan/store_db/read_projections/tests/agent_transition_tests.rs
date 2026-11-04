use super::*;
use crate::plan::{PlanStatus, TaskStatus};

#[test]
fn transition_worker_reads_select_constant_bytes_without_writes() {
    let mut previous = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let bound = database
            .transition_worker_is_task_bound_projection("worker")
            .unwrap();
        let bindings = database
            .transition_worker_task_bindings_projection("worker")
            .unwrap();
        let owner = database.plan_identity_projection(&plan.id).unwrap();
        let planner = database.hierarchy_planner_projection(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert!(bound);
        assert_eq!(bindings, vec![(plan.id.clone(), plan.tasks[0].id.clone())]);
        assert_eq!(owner.owner, plan.owner);
        assert_eq!(planner, plan.planner);
        assert_eq!(metrics.queries, 4);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(bound, bindings)).unwrap().len();
        println!("fixture={} consumers=worker_is_task_bound,worker_task_bindings,plan_owner,planner_incumbent,transition_plan_planner tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, metrics.queries, metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn transition_worker_reads_preserve_terminal_lane_retired_attempt_and_archived_plan_rules() {
    for variant in 0..9 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = Plan::new(
            "Transition".into(),
            directory.path().display().to_string(),
            OrchestratorRef::default(),
        );
        let mut task =
            crate::service_plan::load_audit::task("task_exact", TaskStatus::InProgress, None);
        let mut attempt = crate::service_plan::load_audit::attempt("current", "worker");
        match variant {
            0 => task.assigned_tab_id = Some("worker".into()),
            1 => task.lane.tab_id = Some("worker".into()),
            2 => {
                task.status = TaskStatus::Done;
                task.assigned_tab_id = Some("worker".into());
            }
            3 => {
                task.status = TaskStatus::Cancelled;
                task.lane.tab_id = Some("worker".into());
            }
            4 => {
                task.status = TaskStatus::Done;
                task.execution_attempts.push(attempt);
            }
            5 => {
                attempt.retired_at = Some(1);
                task.execution_attempts.push(attempt);
            }
            6 => {
                attempt.closed_at = Some(1);
                task.execution_attempts.push(attempt);
            }
            7 => {
                plan.status = PlanStatus::Archived;
                task.assigned_tab_id = Some("worker".into());
            }
            _ => {}
        }
        let expected_bound = (!task.status.is_terminal()
            && (task.assigned_tab_id.as_deref() == Some("worker")
                || task.lane.tab_id.as_deref() == Some("worker")))
            || task
                .execution_attempts
                .iter()
                .any(|a| a.executor_tab_id == "worker" && a.closed_at.is_none());
        let expected_bindings =
            if !task.status.is_terminal() && task.assigned_tab_id.as_deref() == Some("worker") {
                vec![(plan.id.clone(), task.id.clone())]
            } else {
                vec![]
            };
        plan.tasks.push(task);
        let database = fixtures::database(directory.path(), &plan);
        assert_eq!(
            database
                .transition_worker_is_task_bound_projection("worker")
                .unwrap(),
            expected_bound,
            "variant={variant}"
        );
        assert_eq!(
            database
                .transition_worker_task_bindings_projection("worker")
                .unwrap(),
            expected_bindings,
            "variant={variant}"
        );
        assert!(!database
            .transition_worker_is_task_bound_projection("missing")
            .unwrap());
        database
            .connection
            .lock()
            .execute_batch("DROP TABLE plan_task_attempts")
            .unwrap();
        assert!(database
            .transition_worker_is_task_bound_projection("worker")
            .is_err());
        database
            .connection
            .lock()
            .execute_batch("DROP TABLE plans")
            .unwrap();
        assert!(database
            .transition_worker_task_bindings_projection("worker")
            .is_err());
    }
}

#[test]
fn transition_worker_bindings_preserve_plan_and_task_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Transition".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "a".into();
    plan.tasks = vec![
        crate::service_plan::load_audit::task("task_z", TaskStatus::Paused, Some("worker")),
        crate::service_plan::load_audit::task("task_a", TaskStatus::Blocked, Some("worker")),
    ];
    let database = fixtures::database(directory.path(), &plan);
    let mut newer = plan.clone();
    newer.id = "z".into();
    newer.updated_at += 1;
    database.persist_plan_rows(&newer, None).unwrap();
    assert_eq!(
        database
            .transition_worker_task_bindings_projection("worker")
            .unwrap(),
        vec![
            ("z".into(), "task_z".into()),
            ("z".into(), "task_a".into()),
            ("a".into(), "task_z".into()),
            ("a".into(), "task_a".into())
        ]
    );
}
