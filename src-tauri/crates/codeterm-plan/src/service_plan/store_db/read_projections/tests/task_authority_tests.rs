use super::*;
use crate::plan::{PlanStatus, TaskExecutorTermination, TaskStatus};

fn authority_plan(root: &std::path::Path) -> Plan {
    let mut plan = Plan::new(
        "Authority".into(),
        root.display().to_string(),
        OrchestratorRef::default(),
    );
    let mut task =
        crate::service_plan::load_audit::task("task_exact", TaskStatus::Paused, Some("worker"));
    task.handle = Some("named".into());
    task.key = Some("COD-1".into());
    task.interruption.owner_epoch = 7;
    task.interruption.owner_tab_id = Some("worker".into());
    task.interruption.owner_session_id = Some("session".into());
    let mut attempt = crate::service_plan::load_audit::attempt("current", "worker");
    attempt.session_id = Some("session".into());
    attempt.session_binding_nonce = Some("nonce".into());
    attempt.executor_termination = Some(TaskExecutorTermination {
        attempt_id: attempt.id.clone(),
        executor_tab_id: attempt.executor_tab_id.clone(),
        session_id: attempt.session_id.clone(),
        session_binding_nonce: attempt.session_binding_nonce.clone(),
        confirmed_at: 9,
    });
    task.execution_attempts = vec![attempt];
    plan.tasks = vec![task];
    plan
}

#[test]
fn owner_epoch_selects_exact_aliases_and_preserves_partial_interruption_fields() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let expected = database.task_owner_epoch_projection("task_exact").unwrap();
    assert_eq!(expected.owner_epoch, 7);
    assert_eq!(expected.owner_session_id.as_deref(), Some("session"));
    for reference in ["NAMED", "cod-1"] {
        assert_eq!(
            database.task_owner_epoch_projection(reference).unwrap(),
            expected
        );
    }
    let mut collision = plan.tasks[0].clone();
    collision.id = "collision".into();
    collision.handle = Some("task_exact".into());
    plan.tasks.push(collision);
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database.task_owner_epoch_projection("task_exact").unwrap(),
        expected
    );
    assert!(matches!(
        database.task_owner_epoch_projection("COD-1"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption',json(?1)) WHERE task_id='task_exact'", [r#"{"owner_epoch":8,"owner_tab_id":"worker","handoff_required":true}"#]).unwrap();
    let partial = database.task_owner_epoch_projection("task_exact").unwrap();
    assert_eq!(partial.owner_epoch, 8);
    assert!(partial.handoff_required);
    assert_eq!(partial.owner_session_id, None);
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.owner_epoch','invalid') WHERE task_id='task_exact'", []).unwrap();
    assert!(database.task_owner_epoch_projection("task_exact").is_err());
    plan.status = PlanStatus::Archived;
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(matches!(
        database.task_owner_epoch_projection("task_exact"),
        Err(PlanError::TaskNotFound)
    ));
}

#[test]
fn skill_admission_rejects_stale_or_orphaned_and_duplicate_attempts() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let expected = Some((plan.id.clone(), "task_exact".into(), "current".into()));
    assert_eq!(
        database
            .current_skill_attempt("worker", "session", "nonce")
            .unwrap(),
        expected
    );
    for (session, nonce) in [("stale", "nonce"), ("session", "stale")] {
        assert!(database
            .current_skill_attempt("worker", session, nonce)
            .is_err());
    }
    for (closed, retired) in [(Some(3), None), (None, Some(3))] {
        plan.tasks[0].execution_attempts[0].closed_at = closed;
        plan.tasks[0].execution_attempts[0].retired_at = retired;
        database.persist_plan_rows(&plan, None).unwrap();
        assert!(database
            .current_skill_attempt("worker", "session", "nonce")
            .is_err());
    }
    plan.tasks[0].execution_attempts[0].closed_at = None;
    plan.tasks[0].execution_attempts[0].retired_at = None;
    plan.tasks[0].assigned_tab_id = None;
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(database
        .current_skill_attempt("worker", "session", "nonce")
        .is_err());
    plan.tasks[0].execution_attempts[0].competition_group_id = Some("competition".into());
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .current_skill_attempt("worker", "session", "nonce")
            .unwrap(),
        expected
    );
    let mut duplicate = plan.tasks[0].execution_attempts[0].clone();
    duplicate.id = "duplicate".into();
    plan.tasks[0].execution_attempts.push(duplicate);
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(database
        .current_skill_attempt("worker", "session", "nonce")
        .is_err());
    plan.tasks[0].status = TaskStatus::Done;
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .current_skill_attempt("worker", "session", "nonce")
            .unwrap(),
        None
    );
}

#[test]
fn executor_termination_requires_exact_attempt_and_receipt_identity() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let source = plan.tasks[0].execution_attempts[0].clone();
    let database = fixtures::database(directory.path(), &plan);
    assert!(database
        .executor_termination_confirmed(&plan.id, "named", &source)
        .unwrap());
    for field in ["id", "tab", "session", "nonce", "peer"] {
        let mut changed = source.clone();
        match field {
            "id" => changed.id = "other".into(),
            "tab" => changed.executor_tab_id = "other".into(),
            "session" => changed.session_id = Some("other".into()),
            "nonce" => changed.session_binding_nonce = Some("other".into()),
            _ => changed.mesh_peer = Some("remote".into()),
        }
        assert!(!database
            .executor_termination_confirmed(&plan.id, "task_exact", &changed)
            .unwrap());
    }
    plan.tasks[0].execution_attempts[0]
        .executor_termination
        .as_mut()
        .unwrap()
        .session_binding_nonce = Some("stale".into());
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(!database
        .executor_termination_confirmed(&plan.id, "task_exact", &source)
        .unwrap());
    assert!(matches!(
        database.executor_termination_confirmed("missing", "task_exact", &source),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.executor_termination_confirmed(&plan.id, "missing", &source),
        Err(PlanError::TaskNotFound)
    ));
}

#[test]
fn authority_reads_ignore_unrequested_prose_and_selected_and_unrelated_history_growth() {
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
        plan.tasks[0].interruption.owner_epoch = 7;
        plan.tasks[0].interruption.owner_tab_id = Some("worker".into());
        plan.tasks[0].interruption.owner_session_id = Some("session".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let source = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = source.clone();
            old.id = format!("unselected-{index}");
            old.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(old);
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let owner = database.task_owner_epoch_projection("task_0000").unwrap();
        let current = database
            .current_skill_attempt("worker", "session", "nonce")
            .unwrap();
        let terminated = database
            .executor_termination_confirmed(&plan.id, "task_0000", &source)
            .unwrap();
        assert_eq!(owner.owner_epoch, 7);
        assert_eq!(
            current,
            Some((plan.id.clone(), "task_0000".into(), "current".into()))
        );
        assert!(!terminated);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(owner, current, terminated))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 5);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=task_authority tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn authority_identity_and_termination_reads_observe_committed_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let plan = authority_plan(directory.path());
    let source = plan.tasks[0].execution_attempts[0].clone();
    let database = fixtures::database(directory.path(), &plan);
    let path = database.path();
    let id = plan.id.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..32 {
            let (epoch, owner, nonce) = if index % 2 == 0 {
                (8, "other", "other")
            } else {
                (7, "worker", "nonce")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.owner_epoch',?2,'$.interruption.owner_tab_id',?3) WHERE plan_id=?1",params![id,epoch,owner]).unwrap();
            tx.execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_binding_nonce',?2,'$.executor_termination.session_binding_nonce',?2) WHERE plan_id=?1",params![id,nonce]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..32 {
        barrier.wait();
        let owner = database.task_owner_epoch_projection("task_exact").unwrap();
        assert!(matches!(
            (owner.owner_epoch, owner.owner_tab_id.as_deref()),
            (7, Some("worker")) | (8, Some("other"))
        ));
        if let Ok(Some(current)) = database.current_skill_attempt("worker", "session", "nonce") {
            assert_eq!(
                current,
                (plan.id.clone(), "task_exact".into(), "current".into())
            );
        }
        database
            .executor_termination_confirmed(&plan.id, "task_exact", &source)
            .unwrap();
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_task_attempts")
        .unwrap();
    assert!(database
        .current_skill_attempt("worker", "session", "nonce")
        .is_err());
    assert!(database
        .executor_termination_confirmed(&plan.id, "task_exact", &source)
        .is_err());
}

#[test]
fn context_files_select_only_requested_task_metadata_across_growth_variants() {
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
        plan.tasks[0].files.edit = vec!["src/owned.rs".into()];
        plan.tasks[0].files.read = vec!["docs/contract.md".into()];
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let source = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = source.clone();
            old.id = format!("unselected-{index}");
            old.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(old);
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let files = database
            .task_context_files_projection("fixture-0000")
            .unwrap();
        assert_eq!(files.plan_id, plan.id);
        assert_eq!(files.task_id, "task_0000");
        assert_eq!(files.files, plan.tasks[0].files);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&files).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 256);
        assert!(decoded < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=task_context_files tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        assert!(matches!(
            database.task_context_files_projection("missing"),
            Err(PlanError::TaskNotFound)
        ));
        database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.files.edit',42) WHERE task_id='task_0000'",[]).unwrap();
        assert!(database.task_context_files_projection("task_0000").is_err());
    }
}
