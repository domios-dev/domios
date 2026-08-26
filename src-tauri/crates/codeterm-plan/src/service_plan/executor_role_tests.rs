//! The back door: `plan task update` defines `--executor-role` a second time, so
//! requiring it at `add` alone would leave a guarded front door with an open
//! back one. The guard lives where the CLI, HTTP, and socket update paths
//! converge -- `update_task_with_lane` -- not on any single entrance.

use super::*;
use crate::plan::OrchestratorRef;

fn store_with_roled_task() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Contract".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Owned work".into(),
                executor_role: Some("rust-backend-dev".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, plan.id, task)
}

fn stored_role(store: &PlanStore, plan_id: &str, task_id: &str) -> Option<String> {
    store
        .get(plan_id)
        .unwrap()
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .unwrap()
        .executor_role
        .clone()
}

fn update_role(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    role: Option<Option<&str>>,
    now_ms: u64,
) -> Result<(), PlanError> {
    store.update_task(
        plan_id, task_id, None, None, None, None, None, None, None, None, None, role, None, None,
        None, &[], None, None, now_ms,
    )
}

#[test]
fn executor_role_required_update_cannot_clear_the_role_to_empty() {
    let (_dir, store, plan_id, task_id) = store_with_roled_task();

    // null (HTTP/socket), "" and whitespace (CLI) are the three ways to empty it.
    for blanking in [None, Some(""), Some("   ")] {
        let err = update_role(&store, &plan_id, &task_id, Some(blanking), 3).unwrap_err();
        assert!(
            matches!(err, PlanError::ExecutorRoleRequired),
            "{blanking:?} must be refused, got {err:?}"
        );
        assert_eq!(
            stored_role(&store, &plan_id, &task_id).as_deref(),
            Some("rust-backend-dev"),
            "a task that had a role still has one afterwards"
        );
    }

    // Presence, not vocabulary: replacing one role with a short, unimaginative
    // one is accepted, because any lexical floor is satisfiable by a longer shrug.
    update_role(&store, &plan_id, &task_id, Some(Some("dev")), 4).unwrap();
    assert_eq!(
        stored_role(&store, &plan_id, &task_id).as_deref(),
        Some("dev")
    );

    // An update that does not mention the field leaves it alone.
    update_role(&store, &plan_id, &task_id, None, 5).unwrap();
    assert_eq!(
        stored_role(&store, &plan_id, &task_id).as_deref(),
        Some("dev")
    );
}
