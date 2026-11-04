use super::*;
use crate::plan::RemoteDispatchState;

#[test]
fn remote_projection_preserves_first_id_binding_and_open_recovery_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let attempt = &mut plan.tasks[0].execution_attempts[0];
    attempt.mesh_peer = Some("peer".into());
    attempt.assigned_at = Some(3);
    attempt.remote_dispatch_state = Some(RemoteDispatchState::AwaitingWorkerAck);
    let mut later = attempt.clone();
    later.session_binding_nonce = Some("later".into());
    plan.tasks[0].execution_attempts.push(later);
    let mut closed = plan.tasks[0].execution_attempts[0].clone();
    closed.id = "closed".into();
    closed.closed_at = Some(4);
    let mut retired = closed.clone();
    retired.id = "retired".into();
    retired.closed_at = None;
    retired.retired_at = Some(5);
    let mut local = closed.clone();
    local.id = "local".into();
    local.closed_at = None;
    local.mesh_peer = None;
    plan.tasks[0]
        .execution_attempts
        .extend([closed, retired, local]);
    let database = fixtures::database(directory.path(), &plan);
    let read = database
        .remote_worker_binding_projection(&plan.id, "NAMED", "current")
        .unwrap()
        .unwrap();
    assert_eq!(read.session_binding_nonce.as_deref(), Some("nonce"));
    assert!(read.can_observe("peer", "worker"));
    assert!(!read.can_observe("other", "worker"));
    assert!(!read.can_observe("peer", "other"));
    for id in ["closed", "retired", "local"] {
        assert!(!database
            .remote_worker_binding_projection(&plan.id, "task_exact", id)
            .unwrap()
            .unwrap()
            .can_observe("peer", "worker"));
    }
    let recovery = database
        .remote_recovery_attempts_projection(&plan.id, "task_exact")
        .unwrap();
    assert_eq!(recovery.len(), 2);
    assert_eq!(recovery[0].binding, read);
    assert_eq!(
        recovery[1].binding.session_binding_nonce.as_deref(),
        Some("later")
    );
    assert_eq!(recovery[0].assigned_at, Some(3));
    assert_eq!(
        recovery[0].remote_dispatch_state,
        Some(RemoteDispatchState::AwaitingWorkerAck)
    );
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.report',json('{}')) WHERE task_id='task_exact' AND attempt_id='current'",[]).unwrap();
    assert!(database
        .remote_recovery_attempts_projection(&plan.id, "task_exact")
        .unwrap()
        .is_empty());
    assert_eq!(
        database
            .remote_worker_binding_projection(&plan.id, "task_exact", "current")
            .unwrap()
            .unwrap(),
        read
    );
    assert!(database
        .remote_worker_binding_projection(&plan.id, "task_exact", "missing")
        .unwrap()
        .is_none());
    assert!(matches!(
        database.remote_recovery_attempts_projection(&plan.id, "missing"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_binding_nonce',42) WHERE task_id='task_exact' AND attempt_id='current'",[]).unwrap();
    assert!(database
        .remote_worker_binding_projection(&plan.id, "task_exact", "current")
        .is_err());
}

#[test]
fn remote_projection_bytes_ignore_unselected_body_history_and_report_growth() {
    let mut prior = None;
    for (strides, attempts, body, history, report) in [
        (0, 1, 0, 0, 0),
        (2048, 1, 0, 0, 0),
        (0, 2048, 0, 0, 0),
        (0, 1, 32768, 0, 0),
        (0, 1, 0, 2048, 0),
        (0, 1, 0, 0, 32768),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        let attempt = &mut plan.tasks[0].execution_attempts[0];
        attempt.mesh_peer = Some("peer".into());
        attempt.assigned_at = Some(3);
        attempt.remote_dispatch_state = Some(RemoteDispatchState::AwaitingWorkerAck);
        let original = attempt.clone();
        for index in 0..history {
            let mut old = original.clone();
            old.id = format!("closed-{index}");
            old.closed_at = Some(2);
            plan.tasks[0].execution_attempts.push(old);
        }
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        if report > 0 {
            database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.report',?1) WHERE task_id='task_0001'",["x".repeat(report)]).unwrap();
        }
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = database
            .remote_worker_binding_projection(&plan.id, "task_0000", "current")
            .unwrap()
            .unwrap();
        let recovery = database
            .remote_recovery_attempts_projection(&plan.id, "task_0000")
            .unwrap();
        assert_eq!(recovery.len(), 1);
        assert_eq!(recovery[0].binding, read);
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(read, recovery)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=remote_attempt tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} unrelated_report_bytes={report} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.assigned_at','invalid') WHERE task_id='task_0000' AND attempt_id='current'",[]).unwrap();
        assert!(database
            .remote_recovery_attempts_projection(&plan.id, "task_0000")
            .is_err());
        assert!(database
            .remote_worker_binding_projection(&plan.id, "task_0000", "current")
            .is_ok());
    }
}
