use super::*;

#[test]
fn session_model_detected_before_binding_is_replayed_when_attempt_becomes_eligible() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Session model replay".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker session binding".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "worker-tab", None, 3)
        .unwrap();
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();

    assert!(!store
        .update_attempt_session_model_if_unknown(
            &plan.id,
            &task_id,
            "worker-tab",
            &attempt_id,
            "worker-session",
            "worker-nonce",
            "gpt-6-luna",
            4,
        )
        .unwrap());
    store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            "worker-tab",
            None,
            Some("worker-session"),
            Some("worker-nonce"),
            5,
        )
        .unwrap();

    assert!(store
        .update_attempt_session_model_if_unknown(
            &plan.id,
            &task_id,
            "worker-tab",
            &attempt_id,
            "worker-session",
            "worker-nonce",
            "gpt-6-luna",
            6,
        )
        .unwrap());
    assert!(!store
        .update_attempt_session_model_if_unknown(
            &plan.id,
            &task_id,
            "worker-tab",
            &attempt_id,
            "worker-session",
            "worker-nonce",
            "replacement-model",
            7,
        )
        .unwrap());

    let attempt = store
        .show_task(&task_id)
        .unwrap()
        .task
        .execution_attempts
        .into_iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert_eq!(attempt.session_model.as_deref(), Some("gpt-6-luna"));
}

#[test]
fn attempt_model_identity_is_visible_in_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Attempt model identity".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker identity".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "worker-tab", None, 3)
        .unwrap();
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            "worker-tab",
            None,
            Some("worker-session"),
            Some("worker-nonce"),
            4,
        )
        .unwrap();

    assert!(store
        .update_attempt_model_identity(
            &plan.id,
            &task_id,
            "worker-tab",
            &attempt_id,
            "worker-session",
            "worker-nonce",
            Some("sonnet".into()),
            Some("opus".into()),
            Some("opus".into()),
            Some(crate::plan::AttemptModelIdentityMismatch {
                requested_model: "sonnet".into(),
                provider_resolved_model: Some("sonnet".into()),
                actual_model: "opus".into(),
                reason: "runtime_session_model_mismatch".into(),
            }),
            5,
        )
        .unwrap());

    let attempt = store
        .list_task_attempts(&plan.id, &task_id)
        .unwrap()
        .remove(0);
    let serialized = serde_json::to_value(attempt.attempt).unwrap();
    assert_eq!(serialized["requested_model"], "sonnet");
    assert_eq!(serialized["provider_resolved_model"], "opus");
    assert_eq!(serialized["session_model"], "opus");
    assert_eq!(
        serialized["model_identity_mismatch"]["requested_model"],
        "sonnet"
    );
    assert_eq!(
        serialized["model_identity_mismatch"]["actual_model"],
        "opus"
    );
}
