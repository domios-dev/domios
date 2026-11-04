use super::*;

#[test]
fn review_continuation_growth_is_bounded_and_selected_decode_errors_propagate() {
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
        plan.tasks[0].status = TaskStatus::InReview;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].review_round = 2;
        let attempt = &mut plan.tasks[0].execution_attempts[0];
        attempt.acknowledged_at = Some(1);
        attempt.reported_at = Some(2);
        attempt.closed_at = Some(3);
        attempt.continuation.epoch = 4;
        let original = attempt.clone();
        for index in 0..history {
            let mut old = original.clone();
            old.id = format!("closed-{index}");
            old.executor_tab_id = "unrelated".into();
            plan.tasks[0].execution_attempts.push(old);
        }
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = database
            .closed_review_continuation_projection(&plan.id, "task_0000")
            .unwrap()
            .unwrap();
        assert_eq!(read.attempt_id, "current");
        assert_eq!(read.executor_tab_id, "worker");
        assert_eq!(read.session_id, "session");
        assert_eq!(read.epoch, 4);
        assert_eq!(read.review_round, 2);
        assert_eq!(read.state, "executor_gone");
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=closed_review_continuation tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        assert!(database
            .closed_review_continuation_projection("missing", "task_0000")
            .unwrap()
            .is_none());
        assert!(database
            .closed_review_continuation_projection(&plan.id, "missing")
            .unwrap()
            .is_none());
        database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.acknowledged_at','invalid') WHERE task_id='task_0000' AND attempt_id='current'",[]).unwrap();
        assert!(database
            .closed_review_continuation_projection(&plan.id, "task_0000")
            .is_err());
    }
}
