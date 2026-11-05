use super::*;
use crate::plan::TaskExecutionAttempt;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;

#[test]
fn executor_sources_preserve_latest_unclosed_retired_archived_and_ambiguous_bindings() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.status = crate::plan::PlanStatus::Archived;
    let original = plan.tasks[0].execution_attempts[0].clone();
    let mut retired = original.clone();
    retired.id = "retired".into();
    retired.retired_at = Some(3);
    plan.tasks[0].execution_attempts.push(retired);
    let mut closed = original.clone();
    closed.id = "closed".into();
    closed.closed_at = Some(4);
    plan.tasks[0].execution_attempts.push(closed);
    let database = fixtures::database(directory.path(), &plan);
    let rows = database
        .executor_sources_for_tab_projection("worker")
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2.id, "retired");
    for task in ["task_0000", "fixture-0000", "FIXTURE-0000"] {
        assert_eq!(
            database
                .executor_source_projection(&plan.id, task, "current")
                .unwrap()
                .unwrap()
                .id,
            "current"
        );
    }
    assert!(database
        .executor_source_projection(&plan.id, "task_0000", "missing")
        .unwrap()
        .is_none());
    assert!(matches!(
        database.executor_source_projection("missing", "task_0000", "current"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.executor_source_projection(&plan.id, "missing", "current"),
        Err(PlanError::TaskNotFound)
    ));
    plan.tasks[0].execution_attempts.push(TaskExecutionAttempt {
        session_id: Some("duplicate".into()),
        ..original.clone()
    });
    plan.tasks[1].execution_attempts.push(original.clone());
    plan.tasks[2].execution_attempts.push(original);
    database.persist_plan_rows(&plan, None).unwrap();
    let source = database
        .executor_source_projection(&plan.id, "task_0000", "current")
        .unwrap()
        .unwrap();
    assert_eq!(source.session_id.as_deref(), Some("session"));
    assert_eq!(
        database
            .executor_sources_for_tab_projection("worker")
            .unwrap()
            .len(),
        2
    );
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_id',42) WHERE task_id='task_0000'",[]).unwrap();
    assert!(database
        .executor_source_projection(&plan.id, "task_0000", "current")
        .is_err());
    assert!(database
        .executor_sources_for_tab_projection("worker")
        .is_err());
}

#[test]
fn close_bound_worker_executor_fixture_selects_only_identity_without_serialization_or_writes() {
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
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let original = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| TaskExecutionAttempt {
                id: format!("closed-{index}"),
                closed_at: Some(2),
                ..original.clone()
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let exact = database
            .executor_source_projection(&plan.id, "task_0000", "current")
            .unwrap()
            .unwrap();
        let inferred = database
            .executor_sources_for_tab_projection("worker")
            .unwrap();
        assert_eq!(inferred.len(), 1);
        assert_eq!(exact, inferred[0].2);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(exact, inferred)).unwrap().len();
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=close_bound_worker_executor tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
