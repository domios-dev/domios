use super::{NewTask, PlanError, PlanStore};
use crate::plan::{OrchestratorRef, Plan, PlanMode};

fn store_with_execution_plan() -> (tempfile::TempDir, PlanStore, String) {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let mut plan = Plan::new(
        "Name contract".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    (directory, store, plan.id)
}

#[test]
fn tab_name_contract_add_rejects_invalid_executor_names_before_persistence() {
    let (_directory, store, plan_id) = store_with_execution_plan();

    for value in ["rust-backend-dev", "Ada Lovelace"] {
        let error = store
            .add_task(
                &plan_id,
                NewTask {
                    name: "Rejected identity".into(),
                    personality: Some(value.into()),
                    executor_role: Some("rust-backend-dev".into()),
                    ..Default::default()
                },
                2,
            )
            .unwrap_err();
        assert!(matches!(error, PlanError::InvalidExecutorName(_)));
        assert_eq!(error.to_json()["type"], "invalid_executor_name");
        assert_eq!(store.get(&plan_id).unwrap().tasks.len(), 0);
    }
}

#[test]
fn tab_name_contract_update_normalizes_valid_name_and_rejects_invalid_name_atomically() {
    let (_directory, store, plan_id) = store_with_execution_plan();
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "Stable identity".into(),
                personality: Some("Ada".into()),
                executor_role: Some("rust-backend-dev".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .update_task_personality(&plan_id, &task_id, Some(Some("  Hopper  ")), None, 3)
        .unwrap();
    let updated = store.get(&plan_id).unwrap();
    let task = updated
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.personality.as_deref(), Some("Hopper"));
    let updated_at = task.updated_at;

    let error = store
        .update_task_personality(&plan_id, &task_id, Some(Some("Ada Lovelace")), None, 4)
        .unwrap_err();
    assert!(matches!(error, PlanError::InvalidExecutorName(_)));
    let unchanged = store.get(&plan_id).unwrap();
    let task = unchanged
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.personality.as_deref(), Some("Hopper"));
    assert_eq!(task.updated_at, updated_at);
}

#[test]
fn tab_name_contract_batch_rejects_invalid_executor_names_before_persistence() {
    for personality in ["rust-backend-dev", "Ada Lovelace"] {
        let (_directory, store, plan_id) = store_with_execution_plan();
        let request: crate::service_plan::batch::BatchRequest =
            serde_json::from_value(serde_json::json!({
                "ops": [{
                    "op": "upsert_task",
                    "handle": "invalid-identity",
                    "name": "Rejected identity",
                    "personality": personality
                }]
            }))
            .unwrap();

        let error = store.apply_batch(&plan_id, request, 2).unwrap_err();
        assert!(matches!(error, PlanError::InvalidExecutorName(_)));
        assert!(store.get(&plan_id).unwrap().tasks.is_empty());
    }
}
