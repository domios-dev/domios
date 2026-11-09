use super::*;
use crate::service_plan::task_authority_projection::ensure_respawn_source_fields;

#[test]
fn respawn_source_projection_matches_latest_identity_and_retirement_decisions() {
    for closed in [None, Some(0), Some(u64::MAX)] {
        for retired in [None, Some(0), Some(u64::MAX)] {
            let directory = tempfile::tempdir().unwrap();
            let mut plan = authority_plan(directory.path());
            let attempt = &mut plan.tasks[0].execution_attempts[0];
            attempt.closed_at = closed;
            attempt.retired_at = retired;
            let mut older = attempt.clone();
            older.executor_tab_id = "older-worker".into();
            plan.tasks[0].execution_attempts.insert(0, older);
            let database = fixtures::database(directory.path(), &plan);
            let read = database
                .task_respawn_source_projection(&plan.id, "NAMED")
                .unwrap()
                .unwrap();
            assert_eq!(read.attempt_id, "current");
            assert_eq!(read.executor_tab_id, "worker");
            assert_eq!(read.closed, closed.is_some());
            assert_eq!(read.retired, retired.is_some());
            for source in ["current", "old"] {
                let actual = ensure_respawn_source_fields(
                    &read.attempt_id,
                    &read.executor_tab_id,
                    read.closed,
                    read.retired,
                    source,
                )
                .map_err(|error| error.to_string());
                let expected = crate::service_plan::task_attempts::ensure_respawn_source_current(
                    &plan.tasks[0],
                    source,
                )
                .map_err(|error| error.to_string());
                assert_eq!(actual, expected);
                assert_eq!(
                    actual.is_ok(),
                    source == "current" && (retired.is_none() || closed.is_some())
                );
            }
            assert!(database
                .task_respawn_source_projection("missing", "task_exact")
                .unwrap()
                .is_none());
            assert!(database
                .task_respawn_source_projection(&plan.id, "missing")
                .unwrap()
                .is_none());
            database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.retired_at','invalid') WHERE task_id='task_exact'",[]).unwrap();
            assert!(database
                .task_respawn_source_projection(&plan.id, "task_exact")
                .is_err());
        }
    }
}

#[test]
fn respawn_source_fixture_ignores_body_and_unselected_history_growth() {
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
        let read = database
            .task_respawn_source_projection(&plan.id, "task_0000")
            .unwrap()
            .unwrap();
        assert_eq!(read.attempt_id, "current");
        assert!(!read.closed);
        assert!(!read.retired);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 256);
        assert!(decoded < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=respawn_reservation tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
