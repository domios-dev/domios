use super::*;

#[test]
fn selected_startup_authority_matches_held_mutation_decisions() {
    let directory = tempfile::tempdir().unwrap();
    let mut base = authority_plan(directory.path());
    base.tasks[0].status = TaskStatus::Pending;
    let mut variants = vec![base.clone()];
    for field in [
        "acknowledged",
        "reported",
        "report",
        "closed",
        "retired",
        "session",
        "executor",
        "assigned",
        "paused",
        "done",
        "competitive",
        "group_session_missing",
        "group_session_blank",
        "later_group",
        "duplicate",
        "retired_latest",
    ] {
        let mut plan = base.clone();
        let task = &mut plan.tasks[0];
        match field {
            "acknowledged" => task.execution_attempts[0].acknowledged_at = Some(1),
            "reported" => task.execution_attempts[0].reported_at = Some(1),
            "report" => {
                task.execution_attempts[0].report = Some(Default::default());
            }
            "closed" => task.execution_attempts[0].closed_at = Some(1),
            "retired" => task.execution_attempts[0].retired_at = Some(1),
            "session" => task.execution_attempts[0].session_id = Some("foreign".into()),
            "executor" => task.execution_attempts[0].executor_tab_id = "foreign".into(),
            "assigned" => task.assigned_tab_id = None,
            "paused" => task.status = TaskStatus::Paused,
            "done" => task.status = TaskStatus::Done,
            "competitive" => {
                task.assigned_tab_id = None;
                task.execution_attempts[0].competition_group_id = Some("group".into());
            }
            "group_session_missing" => {
                task.execution_attempts[0].competition_group_id = Some("group".into());
                task.execution_attempts[0].session_id = None;
            }
            "group_session_blank" => {
                task.execution_attempts[0].competition_group_id = Some("group".into());
                task.execution_attempts[0].session_id = Some(" \t".into());
            }
            "later_group" => {
                task.execution_attempts[0].competition_group_id = Some("group".into());
                let mut later = task.execution_attempts[0].clone();
                later.id = "later".into();
                later.competition_group_id = Some("new".into());
                task.execution_attempts.push(later);
            }
            "duplicate" => {
                let mut earlier = task.execution_attempts[0].clone();
                earlier.acknowledged_at = Some(2);
                task.execution_attempts.insert(0, earlier);
            }
            "retired_latest" => {
                let mut latest = task.execution_attempts[0].clone();
                latest.id = "retired-latest".into();
                latest.retired_at = Some(1);
                task.execution_attempts.push(latest);
            }
            _ => unreachable!(),
        }
        variants.push(plan);
    }
    let database = fixtures::database(directory.path(), &base);
    for (index, plan) in variants.iter().enumerate() {
        database.persist_plan_rows(plan, None).unwrap();
        let (read, active) = database
            .worker_startup_state_projection(&plan.id, "NAMED", "worker", "current")
            .unwrap();
        assert!(active);
        for session in [None, Some("session"), Some("foreign")] {
            for clear_report in [false, true] {
                let task = &plan.tasks[0];
                let expected = matches!(task.status, TaskStatus::Pending | TaskStatus::InProgress)
                    && crate::service_plan::task_dispatch::startup_attempt(
                        task, "worker", "current",
                    )
                    .is_some_and(|(a, _)| {
                        a.acknowledged_at.is_none()
                            && (!clear_report || (a.reported_at.is_none() && a.report.is_none()))
                            && session
                                .is_none_or(|session| a.session_id.as_deref() == Some(session))
                    });
                assert_eq!(
                    read.owns("worker", session, clear_report),
                    expected,
                    "variant {index} session {session:?} clear {clear_report}"
                );
            }
        }
    }
    base.status = PlanStatus::Archived;
    database.persist_plan_rows(&base, None).unwrap();
    let (read, active) = database
        .worker_startup_state_projection(&base.id, "task_exact", "worker", "current")
        .unwrap();
    assert!(!active);
    assert!(read.owns("worker", None, true));
    assert!(matches!(
        database.worker_startup_state_projection("missing", "task_exact", "worker", "current"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.worker_startup_state_projection(&base.id, "missing", "worker", "current"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.acknowledged_at','invalid') WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .worker_startup_state_projection(&base.id, "task_exact", "worker", "current")
        .is_err());
}

#[test]
fn selected_startup_ownership_materialization_ignores_closed_history_and_body_growth() {
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
        plan.tasks[0].assigned_tab_id = Some("worker".into());
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
        let (read, active) = database
            .worker_startup_state_projection(&plan.id, "task_0000", "worker", "current")
            .unwrap();
        assert!(active);
        assert!(read.owns("worker", Some("session"), true));
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read.owns("worker", Some("session"), true))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 1024);
        assert!(decoded < 1024);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=startup_ownership tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
