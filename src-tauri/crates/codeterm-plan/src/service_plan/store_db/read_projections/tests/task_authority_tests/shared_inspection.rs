use super::*;
use crate::plan::WorkerPlacement;
use crate::service_plan::task_authority_projection::SharedReportRead;

fn reference(task: &crate::plan::Task, reporter: &str) -> Option<SharedReportRead> {
    if task.assigned_tab_id.as_deref() != Some(reporter) {
        return None;
    }
    let attempt = task.execution_attempts.iter().rfind(|attempt| {
        attempt.executor_tab_id == reporter
            && attempt.closed_at.is_none()
            && attempt.retired_at.is_none()
            && (task.interruption.owner_epoch == 0
                || attempt.interruption.owner_epoch == task.interruption.owner_epoch)
    })?;
    if attempt.placement != WorkerPlacement::Shared {
        return None;
    }
    Some(SharedReportRead {
        attempt_id: attempt.id.clone(),
        executor_tab_id: attempt.executor_tab_id.clone(),
        checkout: attempt.checkout_path.clone(),
        start_sha: attempt.shared_start_sha.clone(),
        files: task.files.clone(),
    })
}

fn configure(plan: &mut Plan) {
    plan.tasks[0].assigned_tab_id = Some("worker".into());
    plan.tasks[0].interruption.owner_epoch = 7;
    let attempt = &mut plan.tasks[0].execution_attempts[0];
    attempt.placement = WorkerPlacement::Shared;
    attempt.interruption.owner_epoch = 7;
    attempt.checkout_path = Some("checkout".into());
    attempt.shared_start_sha = Some("base".into());
}

#[test]
fn shared_inspection_preserves_latest_attempt_epoch_and_snapshot_fields() {
    for variant in 0..15 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        configure(&mut plan);
        match variant {
            1 => plan.tasks[0].assigned_tab_id = None,
            2 => plan.tasks[0].execution_attempts[0].interruption.owner_epoch = 8,
            3 => plan.tasks[0].interruption.owner_epoch = 0,
            4 => plan.tasks[0].execution_attempts[0].retired_at = Some(1),
            5 => plan.tasks[0].execution_attempts[0].closed_at = Some(1),
            6 => plan.tasks[0].execution_attempts[0].placement = WorkerPlacement::CanonicalWriter,
            7 => plan.tasks[0].execution_attempts[0].checkout_path = None,
            8 => plan.tasks[0].execution_attempts[0].shared_start_sha = None,
            9..=12 => {
                let mut later = plan.tasks[0].execution_attempts[0].clone();
                later.id = "later".into();
                later.placement = WorkerPlacement::IsolatedLane;
                match variant {
                    10 => later.closed_at = Some(1),
                    11 => later.retired_at = Some(1),
                    12 => later.executor_tab_id = "other".into(),
                    _ => {}
                }
                plan.tasks[0].execution_attempts.push(later);
            }
            13 => {
                plan.tasks[0].interruption.owner_epoch = u64::MAX;
                plan.tasks[0].execution_attempts[0].interruption.owner_epoch = u64::MAX - 1;
            }
            14 => {
                plan.tasks[0].interruption.owner_epoch = u64::MAX;
                plan.tasks[0].execution_attempts[0].interruption.owner_epoch = u64::MAX;
            }
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        assert_eq!(
            database
                .shared_report_projection(&plan.id, "NAMED", "worker")
                .unwrap(),
            reference(&plan.tasks[0], "worker"),
            "variant {variant}"
        );
        let dispatch = database
            .shared_dispatch_projection(&plan.id, "NAMED")
            .unwrap();
        assert_eq!(dispatch.task_id, plan.tasks[0].id);
        assert_eq!(dispatch.files, plan.tasks[0].files);
        let expected = if plan.canonical_project_path.trim().is_empty() {
            &plan.project_path
        } else {
            &plan.canonical_project_path
        };
        assert_eq!(&dispatch.checkout, expected);
        assert_eq!(
            &database.dispatch_checkout_projection(&plan.id).unwrap(),
            expected
        );
        assert!(matches!(
            database.dispatch_checkout_projection("missing"),
            Err(PlanError::NotFound)
        ));
        assert!(matches!(
            database.shared_report_projection("missing", "task_exact", "worker"),
            Err(PlanError::NotFound)
        ));
        assert!(matches!(
            database.shared_dispatch_projection(&plan.id, "missing"),
            Err(PlanError::TaskNotFound)
        ));
    }
}

#[test]
fn shared_inspection_fixture_returns_only_git_snapshot_facts_and_never_writes() {
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
        configure(&mut plan);
        let original = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = original.clone();
            old.id = format!("closed-{index}");
            old.closed_at = Some(2);
            plan.tasks[0].execution_attempts.push(old);
        }
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let dispatch = database
            .shared_dispatch_projection(&plan.id, "task_0000")
            .unwrap();
        let report = database
            .shared_report_projection(&plan.id, "task_0000", "worker")
            .unwrap()
            .unwrap();
        assert_eq!(report.attempt_id, "current");
        assert_eq!(dispatch.files, report.files);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(dispatch, report)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=shared_inspection tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.checkout_path',42) WHERE task_id='task_0000' AND attempt_id='current'",[]).unwrap();
        assert!(database
            .shared_report_projection(&plan.id, "task_0000", "worker")
            .is_err());
    }
}
