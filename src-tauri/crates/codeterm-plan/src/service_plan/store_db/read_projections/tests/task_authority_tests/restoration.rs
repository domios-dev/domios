use super::*;

#[test]
fn lineage_restoration_selects_exact_target_team_and_nonce_with_duplicate_refusal_facts() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.team_id = Some("team".into());
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .task_binding_restore_projection("NAMED", "current")
        .unwrap();
    assert_eq!(read.plan_id, plan.id);
    assert_eq!(read.task_id, "task_exact");
    assert_eq!(read.team_id.as_deref(), Some("team"));
    assert_eq!(read.attempt_count, 1);
    assert_eq!(read.attempt_nonce.as_deref(), Some("nonce"));
    let absent = database
        .task_binding_restore_projection("task_exact", "missing")
        .unwrap();
    assert_eq!(absent.attempt_count, 0);
    assert_eq!(absent.attempt_nonce, None);
    let mut duplicate = plan.tasks[0].execution_attempts[0].clone();
    duplicate.executor_tab_id = "foreign".into();
    duplicate.session_binding_nonce = Some("foreign".into());
    plan.tasks[0].execution_attempts.push(duplicate);
    database.persist_plan_rows(&plan, None).unwrap();
    let duplicate = database
        .task_binding_restore_projection("task_exact", "current")
        .unwrap();
    assert_eq!(duplicate.attempt_count, 2);
    assert_eq!(duplicate.attempt_nonce.as_deref(), Some("nonce"));
    assert!(matches!(
        database.task_binding_restore_projection("missing", "current"),
        Err(PlanError::TaskNotFound)
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.team_id',json('[]')) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(database
        .task_binding_restore_projection("task_exact", "current")
        .is_err());
}

#[test]
fn review_restoration_preserves_scope_epoch_open_order_and_ambiguity() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.status = PlanStatus::Archived;
    plan.tasks[0].status = TaskStatus::InReview;
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .review_binding_restore_projection(&plan.id, "NAMED")
        .unwrap();
    assert_eq!(read.task_id, "task_exact");
    assert_eq!(read.owner_epoch, 7);
    assert_eq!(read.attempts.len(), 1);
    assert_eq!(read.attempts[0].id, "current");
    assert_eq!(read.attempts[0].executor_tab_id, "worker");
    assert_eq!(
        read.attempts[0].session_binding_nonce.as_deref(),
        Some("nonce")
    );
    for (id, closed, retired) in [
        ("closed", Some(2), None),
        ("retired", None, Some(2)),
        ("second", None, None),
        ("third", None, None),
    ] {
        let mut attempt = plan.tasks[0].execution_attempts[0].clone();
        attempt.id = id.into();
        attempt.closed_at = closed;
        attempt.retired_at = retired;
        plan.tasks[0].execution_attempts.push(attempt);
    }
    database.persist_plan_rows(&plan, None).unwrap();
    let read = database
        .review_binding_restore_projection(&plan.id, "task_exact")
        .unwrap();
    assert_eq!(
        read.attempts
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ["current", "second"]
    );
    plan.tasks[0].status = TaskStatus::Done;
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(database
        .review_binding_restore_projection(&plan.id, "task_exact")
        .unwrap()
        .attempts
        .is_empty());
    assert!(matches!(
        database.review_binding_restore_projection("missing", "task_exact"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.review_binding_restore_projection(&plan.id, "missing"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.owner_epoch','invalid') WHERE task_id='task_exact'", []).unwrap();
    assert!(database
        .review_binding_restore_projection(&plan.id, "task_exact")
        .is_err());
}

#[test]
fn restoration_reads_remain_bounded_across_unselected_body_and_history_growth() {
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
        plan.tasks[0].interruption.owner_epoch = 7;
        let old = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut attempt = old.clone();
            attempt.id = format!("closed-{index}");
            attempt.closed_at = Some(2);
            plan.tasks[0].execution_attempts.push(attempt);
        }
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let identity = database
            .task_binding_restore_projection("task_0000", "current")
            .unwrap();
        let review = database
            .review_binding_restore_projection(&plan.id, "task_0000")
            .unwrap();
        assert_eq!(identity.attempt_count, 1);
        assert_eq!(review.attempts.len(), 1);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(identity, review)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 1024);
        assert!(decoded < 1024);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=binding_restoration tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0", fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
