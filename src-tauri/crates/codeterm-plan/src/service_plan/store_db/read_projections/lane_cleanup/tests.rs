use super::*;
use crate::plan::TaskExecutionAttempt;
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::store_db::read_projections::take_metrics;

#[test]
fn lane_cleanup_facts_keep_historical_remote_and_conflicting_lanes_in_first_seen_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 0);
    plan.tasks[0].lane.branch = Some("lane/current".into());
    plan.tasks[0].assigned_tab_id = Some("worker".into());
    let mut old = plan.tasks[0].execution_attempts[0].clone();
    old.id = "old".into();
    old.closed_at = Some(2);
    old.worktree_path = Some("/historical".into());
    old.branch = Some("lane/historical".into());
    old.remote_worktree_path = Some("".into());
    plan.tasks[0].execution_attempts.push(old.clone());
    old.id = "duplicate".into();
    plan.tasks[0].execution_attempts.push(old.clone());
    old.id = "conflict".into();
    old.destination = Some("other".into());
    plan.tasks[0].execution_attempts.push(old);
    let database = fixtures::database(directory.path(), &plan);
    let read = database.lane_cleanup_projection(&plan.id).unwrap();
    assert_eq!(read.tasks.len(), plan.tasks.len());
    assert_eq!(read.tasks[0].lane, plan.tasks[0].lane);
    assert!(read.tasks[0].remote);
    assert_eq!(read.tasks[0].execution_attempts.len(), 2);
    assert_eq!(read.tasks[0].execution_attempts[0].destination, None);
    assert_eq!(
        read.tasks[0].execution_attempts[1].destination.as_deref(),
        Some("other")
    );
    assert!(matches!(
        database.lane_cleanup_projection("missing"),
        Err(PlanError::NotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.lane',42) WHERE task_id='task_0000'",[]).unwrap();
    assert!(database.lane_cleanup_projection(&plan.id).is_err());
}

#[test]
fn dispatch_lane_cleanup_fixture_selects_lane_facts_without_serialization_or_writes() {
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
        plan.tasks[0].lane.branch = Some("lane/current".into());
        plan.tasks[0].execution_attempts[0].branch = Some("lane/historical".into());
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
        let read = database.lane_cleanup_projection(&plan.id).unwrap();
        assert_eq!(read.tasks[0].execution_attempts.len(), 1);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 1_000_000);
        assert!(decoded < 1_000_000);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=dispatch_lane_cleanup tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} duplicate_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
