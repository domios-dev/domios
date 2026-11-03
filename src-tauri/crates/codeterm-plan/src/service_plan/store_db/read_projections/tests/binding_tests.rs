use super::*;
use crate::plan::{PlanMode, PlanStatus, TaskStatus};

#[test]
fn tab_binding_lists_preserve_plan_task_order_and_unclosed_retired_attempts() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Bindings".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_a".into();
    plan.updated_at = 10;
    plan.mode = PlanMode::Paused;
    for (index, status) in [
        TaskStatus::Pending,
        TaskStatus::Paused,
        TaskStatus::InProgress,
        TaskStatus::InReview,
        TaskStatus::Blocked,
        TaskStatus::Done,
        TaskStatus::Cancelled,
    ]
    .into_iter()
    .enumerate()
    {
        let mut task =
            crate::service_plan::load_audit::task(&format!("t{index}"), status, Some("worker"));
        let mut attempt = crate::service_plan::load_audit::attempt("retired", "worker");
        attempt.retired_at = Some(4);
        let mut second = attempt.clone();
        second.id = "retired_second".into();
        task.execution_attempts = vec![attempt, second];
        plan.tasks.push(task);
    }
    let mut newer = plan.clone();
    newer.id = "pl_new".into();
    newer.updated_at = 11;
    newer.tasks.truncate(1);
    let mut tied = plan.clone();
    tied.id = "pl_b".into();
    tied.tasks.truncate(1);
    let mut archived = plan.clone();
    archived.id = "pl_archived".into();
    archived.status = PlanStatus::Archived;
    let database = fixtures::database(directory.path(), &plan);
    for other in [&newer, &tied, &archived] {
        database.persist_plan_rows(other, None).unwrap();
    }
    let expected = vec![
        ("pl_new", "t0"),
        ("pl_a", "t0"),
        ("pl_a", "t1"),
        ("pl_a", "t2"),
        ("pl_a", "t3"),
        ("pl_a", "t4"),
        ("pl_b", "t0"),
    ]
    .into_iter()
    .map(|(p, t)| (p.to_string(), t.to_string()))
    .collect::<Vec<_>>();
    assert_eq!(
        database.unclosed_attempt_bindings("worker").unwrap(),
        expected
    );
    assert_eq!(
        database
            .assigned_task_bindings("worker")
            .unwrap()
            .into_iter()
            .map(|(p, t, _)| (p, t))
            .collect::<Vec<_>>(),
        expected
    );
    assert!(database.unclosed_attempt_bindings("  ").unwrap().is_empty());
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.closed_at',5) WHERE plan_id='pl_new'",[]).unwrap();
    assert_eq!(
        database.unclosed_attempt_bindings("worker").unwrap(),
        expected[1..]
    );
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',NULL) WHERE plan_id='pl_new'",[]).unwrap();
    assert!(database.assigned_task_bindings("worker").is_err());
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_set(row_json,'$.id',NULL) WHERE plan_id='pl_a'",
            [],
        )
        .unwrap();
    assert!(database.unclosed_attempt_bindings("worker").is_err());
}

#[test]
fn tab_binding_lists_ignore_unrelated_bodies_attempts_and_strides() {
    let mut prior = None;
    for (strides, attempts, extra) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[2].body.push_str(&"x".repeat(extra));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let _full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let assigned = database.assigned_task_bindings("worker").unwrap();
        let attempts = database.unclosed_attempt_bindings("worker").unwrap();
        assert_eq!(attempts, vec![(plan.id.clone(), plan.tasks[0].id.clone())]);
        assert_eq!(
            assigned,
            vec![(
                plan.id.clone(),
                plan.tasks[0].id.clone(),
                plan.tasks[0].name.clone()
            )]
        );
        let response_bytes = serde_json::to_vec(&(assigned, attempts)).unwrap().len();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 256);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=tab_binding_lists tasks={} base_bytes={} old_strides={strides} unrelated_attempts={} extra_body={extra} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,plan.tasks[1].execution_attempts.len(),metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior)
        }
        prior = Some(measured);
    }
}

#[test]
fn open_attempt_binding_denies_stale_retired_closed_archived_and_ambiguous_identities() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Binding".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.mode = PlanMode::Paused;
    let mut task = crate::service_plan::load_audit::task("one", TaskStatus::Paused, Some("worker"));
    let mut attempt = crate::service_plan::load_audit::attempt("current", "worker");
    attempt.session_id = Some("session".into());
    attempt.session_binding_nonce = Some("nonce".into());
    task.execution_attempts = vec![attempt.clone()];
    plan.tasks = vec![task];
    let database = fixtures::database(directory.path(), &plan);
    let expected = Some((plan.id.clone(), "one".into(), "current".into()));
    for require_unknown in [false, true] {
        assert_eq!(
            database
                .open_attempt_binding("worker", "session", "nonce", require_unknown)
                .unwrap(),
            expected
        );
    }
    for (tab, session, nonce) in [
        ("other", "session", "nonce"),
        ("worker", "stale", "nonce"),
        ("worker", "session", "stale"),
        ("worker", "session", "  "),
    ] {
        assert_eq!(
            database
                .open_attempt_binding(tab, session, nonce, false)
                .unwrap(),
            None
        );
    }
    plan.tasks[0].execution_attempts[0].session_model = Some("known".into());
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .open_attempt_binding("worker", "session", "nonce", true)
            .unwrap(),
        None
    );
    assert_eq!(
        database
            .open_attempt_binding("worker", "session", "nonce", false)
            .unwrap(),
        expected
    );
    plan.tasks[0].execution_attempts[0].session_model = None;
    for (closed, retired) in [(Some(1), None), (None, Some(1))] {
        plan.tasks[0].execution_attempts[0].closed_at = closed;
        plan.tasks[0].execution_attempts[0].retired_at = retired;
        database.persist_plan_rows(&plan, None).unwrap();
        assert_eq!(
            database
                .open_attempt_binding("worker", "session", "nonce", false)
                .unwrap(),
            None
        );
    }
    plan.tasks[0].execution_attempts[0] = attempt.clone();
    for status in [TaskStatus::Done, TaskStatus::Cancelled] {
        plan.tasks[0].status = status;
        database.persist_plan_rows(&plan, None).unwrap();
        assert_eq!(
            database
                .open_attempt_binding("worker", "session", "nonce", false)
                .unwrap(),
            None
        );
    }
    plan.tasks[0].status = TaskStatus::InProgress;
    let mut duplicate = attempt.clone();
    duplicate.id = "duplicate".into();
    plan.tasks[0].execution_attempts.push(duplicate);
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .open_attempt_binding("worker", "session", "nonce", false)
            .unwrap(),
        None
    );
    plan.tasks[0].execution_attempts = vec![attempt];
    database.persist_plan_rows(&plan, None).unwrap();
    let mut other = plan.clone();
    other.id = "pl_other_binding".into();
    database.persist_plan_rows(&other, None).unwrap();
    assert_eq!(
        database
            .open_attempt_binding("worker", "session", "nonce", false)
            .unwrap(),
        None
    );
    other.status = PlanStatus::Archived;
    database.persist_plan_rows(&other, None).unwrap();
    assert_eq!(
        database
            .open_attempt_binding("worker", "session", "nonce", false)
            .unwrap(),
        expected
    );
}

#[test]
fn open_attempt_materializes_only_two_identity_tuples_with_no_unrelated_history_or_prose() {
    let mut prior = None;
    for (strides, attempts, extra_body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[2].body.push_str(&"x".repeat(extra_body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline_decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let mut responses = Vec::new();
        for require_unknown in [false, true] {
            let result = database
                .open_attempt_binding("worker", "session", "nonce", require_unknown)
                .unwrap();
            assert_eq!(
                result,
                Some((plan.id.clone(), "task_0000".into(), "current".into()))
            );
            responses.push(result);
        }
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&responses).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 1024);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=open_attempt_binding tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} baseline_decoded_json_bytes={baseline_decoded} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn open_attempt_reads_one_committed_identity_and_propagates_invalid_selected_id() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Binding".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let mut task =
        crate::service_plan::load_audit::task("one", TaskStatus::InProgress, Some("worker"));
    let mut attempt = crate::service_plan::load_audit::attempt("current", "worker");
    attempt.session_id = Some("session".into());
    attempt.session_binding_nonce = Some("A".into());
    task.execution_attempts = vec![attempt];
    plan.tasks = vec![task];
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
            let (status, nonce) = if index % 2 == 0 {
                ("archived", "B")
            } else {
                ("active", "A")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET status=?2,header_json=json_set(header_json,'$.status',?2) WHERE id=?1",params![id,status]).unwrap();
            tx.execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_binding_nonce',?2) WHERE plan_id=?1",params![id,nonce]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..32 {
        barrier.wait();
        assert_eq!(
            database
                .open_attempt_binding("worker", "session", "B", false)
                .unwrap(),
            None
        );
        barrier.wait();
    }
    writer.join().unwrap();
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.id',NULL) WHERE plan_id=?1",[&plan.id]).unwrap();
    assert!(database
        .open_attempt_binding("worker", "session", "A", false)
        .is_err());
}
