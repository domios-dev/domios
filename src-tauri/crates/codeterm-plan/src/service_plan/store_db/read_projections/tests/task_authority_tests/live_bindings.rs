use super::*;

#[test]
fn live_binding_read_preserves_latest_executor_even_if_retired_or_task_finished() {
    for variant in 0..14 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        let current = plan.tasks[0].execution_attempts[0].clone();
        match variant {
            1 => plan.status = PlanStatus::Archived,
            2 => plan.tasks[0].status = TaskStatus::Done,
            3 => plan.tasks[0].execution_attempts[0].retired_at = Some(u64::MAX),
            4 => plan.tasks[0].execution_attempts[0].acknowledged_at = Some(u64::MAX),
            5 => plan.tasks[0].execution_attempts[0].interruption.owner_epoch = u64::MAX,
            6 => plan.tasks[0].execution_attempts[0].closed_at = Some(2),
            7 => {
                let mut later = current.clone();
                later.id = "other".into();
                later.executor_tab_id = "other-worker".into();
                plan.tasks[0].execution_attempts.push(later);
            }
            8 => {
                let mut later = current.clone();
                later.id = "later".into();
                later.retired_at = Some(3);
                plan.tasks[0].execution_attempts.push(later);
            }
            9 => {
                let mut later = current.clone();
                later.id = "closed".into();
                later.closed_at = Some(3);
                plan.tasks[0].execution_attempts.push(later);
            }
            10 => {
                let mut later = current.clone();
                later.interruption.owner_epoch = 9;
                plan.tasks[0].execution_attempts.push(later);
            }
            11 => plan.tasks[0].assigned_tab_id = None,
            12 => {
                plan.tasks[0].execution_attempts[0].orchestrator_tab_id =
                    Some("orchestrator".into());
                plan.tasks[0].execution_attempts[0].parent_tab_id = Some("parent".into());
            }
            13 => plan.tasks[0].execution_attempts[0].executor_tab_id = "other-worker".into(),
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        let actual = database.live_task_attempts_projection("worker").unwrap();
        let expected = plan
            .tasks
            .iter()
            .filter_map(|task| {
                task.execution_attempts
                    .iter()
                    .rfind(|attempt| {
                        attempt.executor_tab_id == "worker" && attempt.closed_at.is_none()
                    })
                    .map(|attempt| (task, attempt))
            })
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), expected.len(), "variant {variant}");
        for (actual, (task, attempt)) in actual.iter().zip(expected) {
            assert_eq!(actual.plan_id, plan.id);
            assert_eq!(actual.task_id, task.id);
            assert_eq!(actual.task_handle, task.handle);
            assert_eq!(actual.task_status, task.status);
            assert_eq!(actual.plan_active, plan.status == PlanStatus::Active);
            assert_eq!(actual.attempt.id, attempt.id);
            assert_eq!(actual.attempt.executor_tab_id, attempt.executor_tab_id);
            assert_eq!(actual.attempt.acknowledged_at, attempt.acknowledged_at);
            assert_eq!(actual.attempt.retired_at, attempt.retired_at);
            assert_eq!(actual.attempt.owner_epoch, attempt.interruption.owner_epoch);
            assert_eq!(
                actual.attempt.orchestrator_tab_id,
                attempt.orchestrator_tab_id
            );
            assert_eq!(actual.attempt.parent_tab_id, attempt.parent_tab_id);
        }
    }
}

#[test]
fn live_binding_read_observes_committed_epoch_changes_and_rejects_selected_corruption() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let first = database.live_task_attempts_projection("worker").unwrap();
    plan.tasks[0].execution_attempts[0].interruption.owner_epoch = 99;
    database.persist_plan_rows(&plan, None).unwrap();
    let next = database.live_task_attempts_projection("worker").unwrap();
    assert_ne!(first[0].attempt.owner_epoch, next[0].attempt.owner_epoch);
    assert_eq!(next[0].attempt.owner_epoch, 99);
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.interruption.owner_epoch','invalid')",[]).unwrap();
    assert!(database.live_task_attempts_projection("worker").is_err());
}

#[test]
fn live_binding_fixture_ignores_unselected_attempt_report_history_and_task_body() {
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
        plan.tasks[0].execution_attempts[0].executor_tab_id = "bound-worker".into();
        plan.tasks[0].execution_attempts[0].interruption.owner_epoch = u64::MAX;
        let current = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| {
                let mut old = current.clone();
                old.id = format!("old-{index}");
                old.closed_at = Some(2);
                old
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        if body > 0 {
            plan.tasks[0].execution_attempts.last_mut().unwrap().report =
                Some(crate::plan::TaskExecutionReportSnapshot {
                    message: Some("x".repeat(body)),
                    ..Default::default()
                });
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = database
            .live_task_attempts_projection("bound-worker")
            .unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].attempt.owner_epoch, u64::MAX);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 1024);
        assert!(decoded < 1024);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=live_binding tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
