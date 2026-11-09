use super::*;
use crate::plan::TaskStatus;
use crate::service_plan::{RestartAttemptRead, TaskRestartQuery};

fn query() -> TaskRestartQuery<'static> {
    TaskRestartQuery {
        task_ref: "task_exact",
        attempt_id: "current",
        executor_tab_id: "worker",
        provider: "source",
        session_id: Some("session"),
        nonce: "nonce",
    }
}

#[test]
fn restart_facts_preserve_first_exact_attempt_and_all_action_counts() {
    let directory = tempfile::tempdir().unwrap();
    let mut base = authority_plan(directory.path());
    base.tasks[0].status = TaskStatus::InProgress;
    base.tasks[0].execution_attempts[0].provider = Some("source".into());
    base.tasks[0].execution_attempts[0].acknowledged_at = Some(1);
    let mut variants = vec![base.clone()];
    for (id, tab, provider, session, nonce, closed, retired) in [
        (
            "current", "worker", "source", "session", "nonce", None, None,
        ),
        (
            "current",
            "worker",
            "foreign",
            "session",
            "nonce",
            Some(2),
            None,
        ),
        ("current", "other", "source", "session", "nonce", None, None),
        (
            "other",
            "worker",
            "source",
            "session",
            "nonce",
            Some(2),
            None,
        ),
        (
            "other",
            "worker",
            "source",
            "session",
            "nonce",
            Some(2),
            Some(3),
        ),
        ("other", "worker", "source", "session", "nonce", None, None),
        ("other", "worker", "source", "stale", "stale", Some(2), None),
    ] {
        let mut plan = base.clone();
        let mut other = plan.tasks[0].execution_attempts[0].clone();
        other.id = id.into();
        other.executor_tab_id = tab.into();
        other.provider = Some(provider.into());
        other.session_id = Some(session.into());
        other.session_binding_nonce = Some(nonce.into());
        other.closed_at = closed;
        other.retired_at = retired;
        plan.tasks[0].execution_attempts.insert(0, other);
        variants.push(plan);
    }
    let database = fixtures::database(directory.path(), &base);
    for (index, plan) in variants.iter().enumerate() {
        database.persist_plan_rows(plan, None).unwrap();
        let read = database
            .task_restart_authority_projection(&query())
            .unwrap();
        let task = &plan.tasks[0];
        let first = task
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == "current" && attempt.executor_tab_id == "worker")
            .unwrap();
        assert_eq!(
            read.attempt,
            Some(RestartAttemptRead {
                id: first.id.clone(),
                executor_tab_id: first.executor_tab_id.clone(),
                provider: first.provider.clone(),
                session_id: first.session_id.clone(),
                session_binding_nonce: first.session_binding_nonce.clone(),
                acknowledged_at: first.acknowledged_at,
                closed_at: first.closed_at,
                retired_at: first.retired_at,
                owner_epoch: first.interruption.owner_epoch,
            }),
            "variant {index}"
        );
        assert_eq!(
            read.attempt_id_count,
            task.execution_attempts
                .iter()
                .filter(|attempt| attempt.id == "current")
                .count() as u64
        );
        assert_eq!(
            read.other_open_attempts,
            task.execution_attempts
                .iter()
                .filter(|attempt| attempt.id != "current"
                    && attempt.closed_at.is_none()
                    && attempt.retired_at.is_none())
                .count() as u64
        );
        assert_eq!(
            read.unclosed_attempts,
            task.execution_attempts
                .iter()
                .filter(|attempt| attempt.closed_at.is_none())
                .count() as u64
        );
        assert_eq!(
            read.provider_attempts,
            task.execution_attempts
                .iter()
                .filter(|attempt| attempt.id == "current"
                    && attempt.provider.as_deref() == Some("source"))
                .count() as u64
        );
        assert_eq!(
            read.runtime_attempts,
            task.execution_attempts
                .iter()
                .filter(|attempt| attempt.id == "current"
                    && attempt.executor_tab_id == "worker"
                    && attempt.provider.as_deref() == Some("source")
                    && attempt.session_id.as_deref() == Some("session")
                    && attempt.session_binding_nonce.as_deref() == Some("nonce")
                    && attempt.retired_at.is_none())
                .count() as u64
        );
        assert_eq!(
            read.completed_source_attempts,
            task.execution_attempts
                .iter()
                .filter(|attempt| attempt.executor_tab_id == "worker"
                    && attempt.provider.as_deref() == Some("source")
                    && attempt.session_id.as_deref() == Some("session")
                    && attempt.session_binding_nonce.as_deref() == Some("nonce")
                    && attempt.acknowledged_at.is_some()
                    && attempt.closed_at.is_some()
                    && attempt.retired_at.is_none())
                .count() as u64
        );
    }
    assert!(database
        .task_completed_restart_projection(&query())
        .unwrap()
        .is_none());
    assert!(matches!(
        database.task_restart_authority_projection(&TaskRestartQuery {
            task_ref: "missing",
            ..query()
        }),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.acknowledged_at','invalid') WHERE attempt_id='current'",[]).unwrap();
    assert!(database
        .task_restart_authority_projection(&query())
        .is_err());
}

#[test]
fn runtime_binding_selects_latest_unclosed_before_retirement_filter_and_limit() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let task = plan.tasks[0].clone();
    for index in 1..4 {
        let mut next = task.clone();
        next.id = format!("task_{index}");
        plan.tasks.push(next);
    }
    for task in plan.tasks.iter_mut().take(3) {
        let mut retired = task.execution_attempts[0].clone();
        retired.id = "latest-retired".into();
        retired.retired_at = Some(2);
        task.execution_attempts.push(retired);
    }
    let database = fixtures::database(directory.path(), &plan);
    let unclosed = database
        .runtime_attempt_bindings_projection("worker", false)
        .unwrap();
    assert_eq!(unclosed.len(), 2);
    assert_eq!(unclosed[0].attempt_id, "latest-retired");
    let current = database
        .runtime_attempt_bindings_projection("worker", true)
        .unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].task_id, "task_3");
    plan.status = crate::plan::PlanStatus::Archived;
    plan.tasks[3].status = TaskStatus::Done;
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .runtime_attempt_bindings_projection("worker", true)
            .unwrap(),
        current
    );
}

#[test]
fn completed_authority_and_digest_are_one_committed_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = authority_plan(directory.path());
    let task = &mut first.tasks[0];
    task.status = TaskStatus::Done;
    task.assigned_tab_id = None;
    task.body = "A".into();
    task.interruption.owner_epoch = 7;
    task.interruption.owner_session_id = Some("sessionA".into());
    let attempt = &mut task.execution_attempts[0];
    attempt.provider = Some("source".into());
    attempt.session_id = Some("sessionA".into());
    attempt.session_binding_nonce = Some("A".into());
    attempt.acknowledged_at = Some(1);
    attempt.closed_at = Some(2);
    attempt.interruption.owner_epoch = 7;
    let mut second = first.clone();
    second.tasks[0].body = "B".into();
    second.tasks[0].interruption.owner_epoch = 8;
    second.tasks[0].interruption.owner_session_id = Some("sessionB".into());
    second.tasks[0].execution_attempts[0].session_id = Some("sessionB".into());
    second.tasks[0].execution_attempts[0].session_binding_nonce = Some("B".into());
    second.tasks[0].execution_attempts[0]
        .interruption
        .owner_epoch = 8;
    let database = fixtures::database(directory.path(), &first);
    let request = TaskRestartQuery {
        session_id: Some("sessionA"),
        nonce: "A",
        ..query()
    };
    let digest_a = database
        .task_completed_restart_projection(&request)
        .unwrap()
        .unwrap()
        .digest;
    database.persist_plan_rows(&second, None).unwrap();
    let digest_b = database
        .task_completed_restart_projection(&request)
        .unwrap()
        .unwrap()
        .digest;
    database.persist_plan_rows(&first, None).unwrap();
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..32 {
            let (body, epoch, session) = if index % 2 == 0 {
                ("B", 8, "sessionB")
            } else {
                ("A", 7, "sessionA")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.body',?1,'$.interruption.owner_epoch',?2,'$.interruption.owner_session_id',?3) WHERE task_id='task_exact'",params![body,epoch,session]).unwrap();
            tx.execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_id',?1,'$.session_binding_nonce',?2,'$.interruption.owner_epoch',?3) WHERE task_id='task_exact'",params![session,body,epoch]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..32 {
        barrier.wait();
        let read = database
            .task_completed_restart_projection(&request)
            .unwrap()
            .unwrap();
        let (digest, matching) = if read.authority.owner_epoch == 7 {
            (&digest_a, 1)
        } else {
            assert_eq!(read.authority.owner_epoch, 8);
            (&digest_b, 0)
        };
        assert_eq!(&read.digest, digest);
        assert_eq!(
            read.authority.attempt.as_ref().unwrap().owner_epoch,
            read.authority.owner_epoch
        );
        assert_eq!(read.authority.completed_source_attempts, matching);
        barrier.wait();
    }
    writer.join().unwrap();
}

#[test]
fn live_restart_readers_do_not_scan_body_or_history_for_incomplete_tasks() {
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
        let task = &mut plan.tasks[0];
        task.assigned_tab_id = Some("worker".into());
        task.status = TaskStatus::InProgress;
        task.interruption.owner_epoch = 7;
        task.interruption.owner_tab_id = Some("worker".into());
        task.interruption.owner_session_id = Some("session".into());
        task.execution_attempts[0].provider = Some("source".into());
        task.execution_attempts[0].acknowledged_at = Some(1);
        let source = task.execution_attempts[0].clone();
        for index in 0..history {
            let mut old = source.clone();
            old.id = format!("old-{index}");
            old.executor_tab_id = "unrelated".into();
            old.closed_at = Some(2);
            task.execution_attempts.push(old);
        }
        task.body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let request = TaskRestartQuery {
            task_ref: "task_0000",
            ..query()
        };
        let authority = database
            .task_restart_authority_projection(&request)
            .unwrap();
        let bindings = database
            .runtime_attempt_bindings_projection("worker", true)
            .unwrap();
        let incomplete = database
            .task_completed_restart_projection(&request)
            .unwrap();
        assert!(incomplete.is_none());
        assert_eq!(bindings.len(), 1);
        assert_eq!(authority.runtime_attempts, 1);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(authority, bindings)).unwrap().len();
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=runtime_restart tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
