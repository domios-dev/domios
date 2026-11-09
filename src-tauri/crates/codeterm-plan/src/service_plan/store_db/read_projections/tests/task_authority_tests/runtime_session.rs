use super::*;
use crate::service_plan::RuntimeSessionAttemptRead;

#[test]
fn runtime_session_selects_first_exact_attempt_epoch_and_requested_lane() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].lane.worktree_path = Some("lane".into());
    plan.tasks[0].lane.base = Some("base".into());
    plan.tasks[0].lane.cleanup_hints = vec!["preserved".into()];
    let source = &mut plan.tasks[0].execution_attempts[0];
    source.closed_at = Some(2);
    source.retired_at = Some(3);
    let mut foreign = source.clone();
    foreign.executor_tab_id = "foreign".into();
    foreign.session_id = Some("foreign".into());
    let mut later = source.clone();
    later.session_id = Some("later".into());
    plan.tasks[0].execution_attempts.insert(0, foreign);
    plan.tasks[0].execution_attempts.push(later);
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .task_runtime_session_projection("NAMED", "current", "worker", true)
        .unwrap();
    assert_eq!(read.plan_id, plan.id);
    assert_eq!(read.owner_epoch, 7);
    assert_eq!(read.lane, Some(plan.tasks[0].lane.clone()));
    assert_eq!(
        read.attempt,
        Some(RuntimeSessionAttemptRead {
            session_id: Some("session".into()),
            session_binding_nonce: Some("nonce".into()),
            closed_at: Some(2),
            retired_at: Some(3)
        })
    );
    assert!(database
        .task_runtime_session_projection("task_exact", "missing", "worker", false)
        .unwrap()
        .attempt
        .is_none());
    assert!(database
        .task_runtime_session_projection("task_exact", "current", "worker", false)
        .unwrap()
        .lane
        .is_none());
    assert!(matches!(
        database.task_runtime_session_projection("missing", "current", "worker", false),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.lane',42) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .task_runtime_session_projection("task_exact", "current", "worker", false)
        .is_ok());
    assert!(database
        .task_runtime_session_projection("task_exact", "current", "worker", true)
        .is_err());
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_binding_nonce',42) WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .task_runtime_session_projection("task_exact", "current", "worker", false)
        .is_err());
}

#[test]
fn runtime_session_bytes_ignore_body_history_and_unrequested_lane_growth() {
    for include_lane in [false, true] {
        let mut prior = None;
        for (strides, attempts, body, history, lane_growth) in [
            (0, 1, 0, 0, 0),
            (2048, 1, 0, 0, 0),
            (0, 2048, 0, 0, 0),
            (0, 1, 32768, 0, 0),
            (0, 1, 0, 2048, 0),
            (0, 1, 0, 0, 2048),
        ] {
            if include_lane && lane_growth > 0 {
                continue;
            }
            let directory = tempfile::tempdir().unwrap();
            let mut plan = fixtures::fixture(directory.path(), strides, attempts);
            let original = plan.tasks[0].execution_attempts[0].clone();
            for index in 0..history {
                let mut old = original.clone();
                old.id = format!("closed-{index}");
                old.closed_at = Some(2);
                plan.tasks[0].execution_attempts.push(old);
            }
            plan.tasks[0].body.push_str(&"x".repeat(body));
            plan.tasks[1].body.push_str(&"x".repeat(body));
            plan.tasks[0].lane.cleanup_hints = (0..lane_growth)
                .map(|i| format!("unrequested-{i}"))
                .collect();
            let database = fixtures::database(directory.path(), &plan);
            fixtures::audit(&database);
            take_metrics();
            crate::service_plan::store_sync::take_decoded_row_bytes();
            crate::service_plan::store_rows::take_serialized_row_bytes();
            let read = database
                .task_runtime_session_projection("task_0000", "current", "worker", include_lane)
                .unwrap();
            assert_eq!(
                read.attempt.as_ref().unwrap().session_id.as_deref(),
                Some("session")
            );
            assert_eq!(read.lane.is_some(), include_lane);
            let metrics = take_metrics();
            let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
            let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
            let response = serde_json::to_vec(&read).unwrap().len();
            assert_eq!(metrics.queries, 1);
            assert!(metrics.materialized_bytes < 2048);
            assert!(decoded < 2048);
            assert_eq!(serialized, 0);
            assert!(fixtures::audit_counts(&database).is_empty());
            println!("fixture={} consumer=runtime_session include_lane={include_lane} tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} unrequested_lane_growth={lane_growth} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
            let measured = (metrics, decoded, response);
            if let Some(prior) = &prior {
                assert_eq!(&measured, prior);
            }
            prior = Some(measured);
        }
    }
}
