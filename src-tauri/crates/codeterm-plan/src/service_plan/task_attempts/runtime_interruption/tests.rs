use super::*;
use crate::plan::TaskStatus;
use crate::service_plan::load_audit;

#[test]
fn runtime_loss_and_same_attempt_recovery_are_durable_and_identity_fenced() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let mut task = load_audit::task("task", TaskStatus::InProgress, Some("worker"));
    let mut attempt = load_audit::attempt("attempt", "worker");
    attempt.session_id = Some("session".into());
    attempt.session_binding_nonce = Some("old-nonce".into());
    task.execution_attempts.push(attempt);
    let plan = load_audit::execution_plan(
        directory.path(),
        "runtime interruption",
        "parent",
        vec![task],
    );
    let plan = load_audit::seed(&store, plan);
    assert!(store
        .record_runtime_interruption(
            &plan, "task", "attempt", "worker", "session", "foreign", 3, "lost", 10
        )
        .is_err());
    store
        .record_runtime_interruption(
            &plan,
            "task",
            "attempt",
            "worker",
            "session",
            "old-nonce",
            3,
            "unexpected channel loss",
            11,
        )
        .unwrap();
    store
        .record_runtime_interruption(
            &plan,
            "task",
            "attempt",
            "worker",
            "session",
            "old-nonce",
            3,
            "duplicate",
            12,
        )
        .unwrap();
    let path = store.database_path_for_test();
    drop(store);
    let store = PlanStore::open(path.clone()).unwrap();
    let interrupted = store
        .show_task_attempt(&plan, "task", "attempt")
        .unwrap()
        .attempt;
    assert_eq!(interrupted.interruption.interrupted_at, Some(11));
    assert_eq!(
        interrupted.interruption.reason.as_deref(),
        Some("unexpected channel loss")
    );
    assert_eq!(interrupted.interruption.recovered_at, None);
    assert_eq!(interrupted.closed_at, None);
    store
        .edit_plan_fixture(&plan, 13, |plan| {
            let attempt = &mut plan.tasks[0].execution_attempts[0];
            attempt.session_binding_nonce = Some("new-nonce".into());
            attempt.session_model = Some("live-model-edit".into());
            Ok(())
        })
        .unwrap();
    assert!(store
        .record_runtime_recovery(
            &plan,
            "task",
            "attempt",
            "worker",
            "session",
            "new-nonce",
            "old-nonce",
            2,
            14
        )
        .is_err());
    store
        .record_runtime_recovery(
            &plan,
            "task",
            "attempt",
            "worker",
            "session",
            "new-nonce",
            "old-nonce",
            3,
            15,
        )
        .unwrap();
    assert!(store
        .record_runtime_interruption(
            &plan,
            "task",
            "attempt",
            "worker",
            "session",
            "old-nonce",
            4,
            "late",
            16
        )
        .is_err());
    drop(store);
    let store = PlanStore::open(path).unwrap();
    let recovered = store
        .show_task_attempt(&plan, "task", "attempt")
        .unwrap()
        .attempt;
    assert_eq!(recovered.interruption.interrupted_at, Some(11));
    assert_eq!(recovered.interruption.recovered_at, Some(15));
    assert_eq!(recovered.session_model.as_deref(), Some("live-model-edit"));
    assert_eq!(recovered.session_id.as_deref(), Some("session"));
    assert_eq!(recovered.closed_at, None);
}
