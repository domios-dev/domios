use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;

#[test]
fn provider_switch_context_selects_aliases_completed_status_and_propagates_sql_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.tasks[0].status = TaskStatus::Done;
    plan.tasks[0].intent = Some("intent-token".into());
    let database = fixtures::database(directory.path(), &plan);
    let exact = database
        .provider_switch_task_context_projection("task_0000")
        .unwrap()
        .unwrap();
    assert_eq!(exact.task.status, TaskStatus::Done);
    assert_eq!(exact.task.intent, plan.tasks[0].intent);
    assert_eq!(exact.task.body, plan.tasks[0].body);
    assert_eq!(
        database
            .provider_switch_task_context_projection("FIXTURE-0000")
            .unwrap()
            .unwrap(),
        exact
    );
    assert!(database
        .provider_switch_task_context_projection("missing")
        .unwrap()
        .is_none());
    plan.status = crate::plan::PlanStatus::Archived;
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(database
        .provider_switch_task_context_projection("task_0000")
        .unwrap()
        .is_none());
    plan.status = crate::plan::PlanStatus::Active;
    database.persist_plan_rows(&plan, None).unwrap();
    for field in ["$.body", "$.acceptance_criteria", "$.files", "$.lane"] {
        database.persist_plan_rows(&plan, None).unwrap();
        database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,?1,json('null')) WHERE task_id='task_0000'",[field]).unwrap();
        assert!(
            database
                .provider_switch_task_context_projection("task_0000")
                .is_err(),
            "{field}"
        );
    }
    database.persist_plan_rows(&plan, None).unwrap();
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',42) WHERE task_id='task_0000'",[]).unwrap();
    assert!(database
        .provider_switch_task_context_projection("task_0000")
        .is_err());
}

#[test]
fn provider_switch_input_fixture_selects_task_context_without_serialization_or_writes() {
    let mut prior = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let context = database
            .provider_switch_task_context_projection("task_0000")
            .unwrap()
            .unwrap();
        assert_eq!(context.task.body, plan.tasks[0].body);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&context).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 32768);
        assert!(decoded < 32768);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=provider_switch_input tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_unselected_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
