use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;

#[test]
fn compete_preflight_preserves_advisory_backlog_and_missing_refusals() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.review_policy.wip_limit = 0;
    let store = PlanStore::new_for_test(directory.path());
    store.database.persist_plan_rows(&plan, None).unwrap();
    assert!(store
        .compete_review_capacity_projection(&plan.id, "task_0000")
        .is_ok());
    assert!(matches!(
        store.compete_review_capacity_projection("missing", "task_0000"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        store.compete_review_capacity_projection(&plan.id, "missing"),
        Err(PlanError::TaskNotFound)
    ));
    store.database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.review_policy.credit','bad') WHERE id=?1",[&plan.id]).unwrap();
    assert!(store
        .compete_review_capacity_projection(&plan.id, "task_0000")
        .is_err());
}

#[test]
fn preflight_compete_review_capacity_fixture_selects_status_and_backlog_without_serialization_or_writes(
) {
    let mut prior = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let store = PlanStore::new_for_test(directory.path());
        store.database.persist_plan_rows(&plan, None).unwrap();
        fixtures::audit(&store.database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        store
            .compete_review_capacity_projection(&plan.id, "task_0000")
            .unwrap();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert!(metrics.queries > 0);
        assert!(metrics.materialized_bytes < 1024);
        assert!(decoded < 1024);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&store.database).is_empty());
        println!("fixture={} consumer=preflight_compete_review_capacity tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
