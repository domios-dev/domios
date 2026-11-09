use super::*;
use crate::service_plan::RuntimeAttemptRestoreQuery;

fn query() -> RuntimeAttemptRestoreQuery<'static> {
    RuntimeAttemptRestoreQuery {
        task_ref: "task_exact",
        executor_tab_id: "worker",
        provider: Some("provider"),
        session_id: Some("session"),
        nonce: Some("nonce"),
        reported_nonce: Some("nonce"),
    }
}

#[test]
fn runtime_restore_selects_own_open_ids_and_preserves_duplicate_id_source_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let original = plan.tasks[0].execution_attempts[0].clone();
    plan.tasks[0].execution_attempts[0].provider = Some("foreign".into());
    plan.tasks[0].execution_attempts[0].acknowledged_at = None;
    let mut source = original.clone();
    source.executor_tab_id = "foreign".into();
    source.closed_at = Some(2);
    source.provider = Some("provider".into());
    source.acknowledged_at = Some(1);
    plan.tasks[0].execution_attempts.push(source);
    for (id, tab, closed, retired) in [
        ("closed", "worker", Some(2), None),
        ("retired", "worker", None, Some(2)),
        ("other", "foreign", None, None),
        ("second", "worker", None, None),
    ] {
        let mut attempt = original.clone();
        attempt.id = id.into();
        attempt.executor_tab_id = tab.into();
        attempt.closed_at = closed;
        attempt.retired_at = retired;
        plan.tasks[0].execution_attempts.push(attempt);
    }
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .runtime_attempt_restore_projection(&query())
        .unwrap();
    let expected = plan.tasks[0]
        .execution_attempts
        .iter()
        .filter(|a| {
            a.executor_tab_id == "worker" && a.closed_at.is_none() && a.retired_at.is_none()
        })
        .map(|a| a.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(read.candidates, expected);
    assert_eq!(read.candidates, ["current", "second"]);
    assert_eq!(read.restart_acknowledged_at, Some(1));
    plan.tasks[0].execution_attempts.remove(1);
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .runtime_attempt_restore_projection(&query())
            .unwrap()
            .restart_acknowledged_at,
        None
    );
    assert!(matches!(
        database.runtime_attempt_restore_projection(&RuntimeAttemptRestoreQuery {
            task_ref: "missing",
            ..query()
        }),
        Err(PlanError::TaskNotFound)
    ));
}

#[test]
fn runtime_restore_source_requires_acknowledged_exact_provider_session_and_nonce() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].execution_attempts[0].provider = Some("provider".into());
    plan.tasks[0].execution_attempts[0].acknowledged_at = Some(1);
    let database = fixtures::database(directory.path(), &plan);
    for request in [
        query(),
        RuntimeAttemptRestoreQuery {
            provider: None,
            ..query()
        },
        RuntimeAttemptRestoreQuery {
            provider: Some("foreign"),
            ..query()
        },
        RuntimeAttemptRestoreQuery {
            session_id: None,
            ..query()
        },
        RuntimeAttemptRestoreQuery {
            session_id: Some("foreign"),
            ..query()
        },
        RuntimeAttemptRestoreQuery {
            nonce: None,
            ..query()
        },
        RuntimeAttemptRestoreQuery {
            nonce: Some("foreign"),
            ..query()
        },
        RuntimeAttemptRestoreQuery {
            reported_nonce: None,
            ..query()
        },
        RuntimeAttemptRestoreQuery {
            reported_nonce: Some("foreign"),
            ..query()
        },
    ] {
        let attempt = &plan.tasks[0].execution_attempts[0];
        let expected = attempt.acknowledged_at.is_some()
            && request.provider.is_some()
            && attempt.provider.as_deref() == request.provider
            && request.session_id.is_some()
            && attempt.session_id.as_deref() == request.session_id
            && request.nonce.is_some()
            && attempt.session_binding_nonce.as_deref() == request.nonce
            && request.reported_nonce == request.nonce;
        let read = database
            .runtime_attempt_restore_projection(&request)
            .unwrap();
        assert_eq!(read.restart_acknowledged_at.is_some(), expected);
    }
    plan.tasks[0].execution_attempts[0].acknowledged_at = None;
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .runtime_attempt_restore_projection(&query())
            .unwrap()
            .restart_acknowledged_at,
        None
    );
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.acknowledged_at','invalid') WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .runtime_attempt_restore_projection(&query())
        .is_err());
}

#[test]
fn runtime_restore_bytes_exclude_unselected_bodies_and_closed_history() {
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
        plan.tasks[0].execution_attempts[0].provider = Some("provider".into());
        plan.tasks[0].execution_attempts[0].acknowledged_at = Some(1);
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
        let read = database
            .runtime_attempt_restore_projection(&RuntimeAttemptRestoreQuery {
                task_ref: "task_0000",
                ..query()
            })
            .unwrap();
        assert_eq!(read.candidates, ["current"]);
        assert_eq!(read.restart_acknowledged_at, Some(1));
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 256);
        assert!(decoded < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=runtime_attempt_restore tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
