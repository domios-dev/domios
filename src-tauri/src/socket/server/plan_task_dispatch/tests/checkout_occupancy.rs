use super::super::lane_safety::preflight_checkout_occupancy;
use crate::plan::{OrchestratorRef, TaskExecutionAttempt};
use crate::service::plan::{NewTask, PlanStore};

fn plan_with_attempt(
    closed_at: Option<u64>,
    worktree_path: Option<String>,
) -> (tempfile::TempDir, crate::plan::Plan, String, String) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let created = store
        .create_plan_for_team(
            "Collision".into(),
            root.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let occupying_task_id = store
        .add_task(
            &created.id,
            NewTask {
                name: "occupant".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let dispatched_task_id = store
        .add_task(
            &created.id,
            NewTask {
                name: "second".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let mut plan = store.get(&created.id).unwrap();
    plan.tasks
        .iter_mut()
        .find(|task| task.id == occupying_task_id)
        .unwrap()
        .execution_attempts
        .push(TaskExecutionAttempt {
            id: "attempt_live".into(),
            executor_tab_id: "tab-live".into(),
            worktree_path,
            closed_at,
            ..Default::default()
        });
    (root, plan, occupying_task_id, dispatched_task_id)
}

#[test]
fn dispatch_refuses_occupied_checkout() {
    let (root, plan, occupying_task_id, dispatched_task_id) = plan_with_attempt(None, None);
    let checkout = root.path().display().to_string();
    #[cfg(windows)]
    assert_ne!(plan.canonical_project_path, checkout);
    assert_eq!(
        codeterm_paths::normalize_cwd(&plan.canonical_project_path),
        codeterm_paths::normalize_cwd(&checkout)
    );
    let error = preflight_checkout_occupancy(&plan, &dispatched_task_id, &checkout).unwrap_err();

    assert_eq!(error.code, -32011);
    assert!(
        error.message.contains(&occupying_task_id),
        "{}",
        error.message
    );
    assert!(error.message.contains("attempt_live"), "{}", error.message);
    assert!(error.message.contains(&checkout), "{}", error.message);
    assert!(
        error.message.contains(&format!(
            "codeterm plan task update {} {} --worktree",
            plan.id, dispatched_task_id
        )),
        "{}",
        error.message
    );
    assert!(error.message.contains("--branch"), "{}", error.message);
    assert!(error.message.contains("--base"), "{}", error.message);
    let data = error.data.unwrap();
    assert_eq!(data["type"], "checkout_occupied");
    assert_eq!(data["occupying_task_id"], occupying_task_id);
    assert_eq!(data["occupying_attempt_id"], "attempt_live");
    assert_eq!(data["checkout_path"], checkout);
    let update_command = data["update_command"].as_str().unwrap();
    assert!(!update_command.contains("<isolated-"), "{update_command}");
    assert!(!update_command.contains("<base-ref>"), "{update_command}");
    #[cfg(windows)]
    assert_eq!(
        update_command,
        format!(
            "codeterm plan task update {} {} --worktree (Read-Host 'Existing isolated worktree path') --branch (Read-Host 'Existing isolated branch') --base (Read-Host 'Base ref')",
            plan.id, dispatched_task_id
        )
    );
    #[cfg(not(windows))]
    assert!(update_command.contains("IFS= read -r value </dev/tty"));
    assert_eq!(
        data["dispatch_command"],
        format!(
            "codeterm plan task dispatch {} {}",
            plan.id, dispatched_task_id
        )
    );
}

#[test]
fn free_checkout_does_not_conflict() {
    let (root, mut plan, _, dispatched_task_id) = plan_with_attempt(Some(10), None);
    plan.tasks
        .iter_mut()
        .for_each(|task| task.execution_attempts.clear());
    let checkout = root.path().display().to_string();

    preflight_checkout_occupancy(&plan, &dispatched_task_id, &checkout).unwrap();
}

#[test]
fn closed_attempt_history_does_not_occupy_checkout() {
    let (root, mut plan, occupying_task_id, dispatched_task_id) = plan_with_attempt(Some(10), None);
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == occupying_task_id)
        .unwrap();
    for index in 1..100 {
        task.execution_attempts.push(TaskExecutionAttempt {
            id: format!("attempt_closed_{index}"),
            executor_tab_id: format!("tab-closed-{index}"),
            closed_at: Some(index),
            ..Default::default()
        });
    }
    let checkout = root.path().display().to_string();

    preflight_checkout_occupancy(&plan, &dispatched_task_id, &checkout).unwrap();
}

#[test]
fn distinct_explicit_checkout_does_not_conflict() {
    let root = tempfile::tempdir().unwrap();
    let occupied = root.path().join("lane-a").display().to_string();
    let distinct = root.path().join("lane-b").display().to_string();
    let (_store_root, plan, _, dispatched_task_id) = plan_with_attempt(None, Some(occupied));

    preflight_checkout_occupancy(&plan, &dispatched_task_id, &distinct).unwrap();
}
