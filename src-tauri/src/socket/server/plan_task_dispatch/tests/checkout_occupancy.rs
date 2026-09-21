use super::super::lane_safety::{isolated_lane_update_command, preflight_checkout_occupancy};
use super::super::{preflight_dispatch_placement, DispatchLanePlacement};
use crate::plan::{ExecutionIntent, OrchestratorRef, TaskExecutionAttempt};
use crate::service::plan::{NewTask, PlanStore};
use crate::service::LaneProtection;

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
    let error = preflight_checkout_occupancy(
        &plan,
        &dispatched_task_id,
        &checkout,
        LaneProtection::Enforced,
    )
    .unwrap_err();

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
    assert_eq!(
        update_command,
        isolated_lane_update_command(&plan.id, &dispatched_task_id)
    );
    assert_eq!(
        data["dispatch_command"],
        format!(
            "codeterm plan task dispatch {} {}",
            plan.id, dispatched_task_id
        )
    );
}

#[test]
fn partial_report_then_redispatch_reuses_its_own_lane() {
    let (root, mut plan, task_id, _) = plan_with_attempt(None, None);
    let checkout = root.path().display().to_string();
    let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
    task.lane.worktree_path = Some(checkout.clone());
    task.lane.branch = Some("lane/partial".into());
    task.execution_attempts[0].worktree_path = Some(checkout.clone());
    task.execution_attempts[0].reported_at = Some(4);

    preflight_checkout_occupancy(
        &plan,
        &task_id,
        &checkout,
        LaneProtection::Enforced,
    )
    .expect("a reported attempt is replaced by redispatch onto its recorded checkout");

    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.lane.worktree_path.as_deref(), Some(checkout.as_str()));
    assert_eq!(task.lane.branch.as_deref(), Some("lane/partial"));
}

#[test]
fn another_task_still_cannot_take_an_occupied_lane() {
    let (root, mut plan, occupying_task_id, dispatched_task_id) = plan_with_attempt(None, None);
    let checkout = root.path().display().to_string();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == occupying_task_id)
        .unwrap();
    task.execution_attempts[0].worktree_path = Some(checkout.clone());
    task.execution_attempts[0].reported_at = Some(4);

    let error = preflight_checkout_occupancy(
        &plan,
        &dispatched_task_id,
        &checkout,
        LaneProtection::Enforced,
    )
    .unwrap_err();

    assert_eq!(error.data.unwrap()["type"], "checkout_occupied");
}

#[test]
fn unreported_attempt_still_occupies_its_own_checkout() {
    let (root, mut plan, task_id, _) = plan_with_attempt(None, None);
    let checkout = root.path().display().to_string();
    let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
    task.execution_attempts[0].worktree_path = Some(checkout.clone());

    let error = preflight_checkout_occupancy(
        &plan,
        &task_id,
        &checkout,
        LaneProtection::Enforced,
    )
    .unwrap_err();

    assert_eq!(error.data.unwrap()["type"], "checkout_occupied");
}

#[test]
fn live_competitive_attempt_still_occupies_its_own_checkout() {
    let (root, mut plan, task_id, _) = plan_with_attempt(None, None);
    let checkout = root.path().display().to_string();
    let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
    task.execution_attempts[0].worktree_path = Some(checkout.clone());
    task.execution_attempts[0].reported_at = Some(4);
    task.execution_attempts[0].competition_group_id = Some("group-live".into());

    let error = preflight_checkout_occupancy(
        &plan,
        &task_id,
        &checkout,
        LaneProtection::Enforced,
    )
    .unwrap_err();

    assert_eq!(error.data.unwrap()["type"], "checkout_occupied");
}

/// Occupancy exists to keep a second WRITER out of a live checkout. The paired
/// Enforced call is what proves the fixture reaches the occupancy branch at all:
/// without it an unconditionally-Ok implementation would pass this test.
#[test]
fn read_only_dispatch_is_not_blocked_by_an_occupied_checkout() {
    let (root, plan, occupying_task_id, dispatched_task_id) = plan_with_attempt(None, None);
    let checkout = root.path().display().to_string();

    let error = preflight_checkout_occupancy(
        &plan,
        &dispatched_task_id,
        &checkout,
        LaneProtection::Enforced,
    )
    .unwrap_err();
    assert_eq!(error.code, -32011);
    assert_eq!(
        error.data.as_ref().unwrap()["occupying_task_id"],
        occupying_task_id
    );

    preflight_checkout_occupancy(
        &plan,
        &dispatched_task_id,
        &checkout,
        LaneProtection::Waived,
    )
    .unwrap();
}

/// The waiver is derived from the dispatch-time intent only; the task-level
/// `TaskExecutionIntent` has no conversion into `LaneProtection`.
#[test]
fn only_a_read_only_dispatch_intent_waives_the_occupancy_guard() {
    assert_eq!(
        LaneProtection::for_dispatch_intent(&ExecutionIntent::ReadOnly),
        LaneProtection::Waived
    );
    assert_eq!(
        LaneProtection::for_dispatch_intent(&ExecutionIntent::Mutating),
        LaneProtection::Enforced
    );
    assert_eq!(
        LaneProtection::for_dispatch_intent(&ExecutionIntent::Unspecified),
        LaneProtection::Enforced
    );
}

#[test]
fn remediation_command_is_non_interactive() {
    let command = isolated_lane_update_command("pl_fixture", "task_fixture");

    assert!(command.contains("pl_fixture"), "{command}");
    assert!(command.contains("task_fixture"), "{command}");
    assert!(!command.contains("Read-Host"), "{command}");
    assert!(!command.contains("/dev/tty"), "{command}");
    assert!(!command.contains("read -r"), "{command}");
    assert!(!command.contains('$'), "{command}");
}

#[test]
fn free_checkout_does_not_conflict() {
    let (root, mut plan, _, dispatched_task_id) = plan_with_attempt(Some(10), None);
    plan.tasks
        .iter_mut()
        .for_each(|task| task.execution_attempts.clear());
    let checkout = root.path().display().to_string();

    preflight_checkout_occupancy(
        &plan,
        &dispatched_task_id,
        &checkout,
        LaneProtection::Enforced,
    )
    .unwrap();
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

    preflight_checkout_occupancy(
        &plan,
        &dispatched_task_id,
        &checkout,
        LaneProtection::Enforced,
    )
    .unwrap();
}

/// codeterm#343: canonical is only a pre-provisioning placeholder here, so a
/// live read-only attempt sitting in it must never refuse this dispatch.
#[test]
fn read_only_attempt_does_not_block_lane_bound_mutating_dispatch() {
    let (root, plan, _read_only_task_id, mutating_task_id) = plan_with_attempt(None, None);
    let canonical_checkout = root.path().display().to_string();
    let placement = DispatchLanePlacement::Provision {
        base_checkout: canonical_checkout.clone(),
    };
    let will_provision_lane = placement.provisions();
    assert!(
        will_provision_lane,
        "a mutating task with no recorded worktree must be provisioned its own isolated lane"
    );
    assert_eq!(placement.spawn_cwd(), canonical_checkout);

    preflight_dispatch_placement(
        &plan,
        &mutating_task_id,
        &canonical_checkout,
        LaneProtection::Enforced,
        will_provision_lane,
    )
    .expect(
        "a task about to be provisioned its own isolated lane must not be refused by \
         occupancy on the canonical placeholder it will never actually use",
    );
}

/// The guard test: occupancy still fires when there is truly no lane to
/// provision, so the fix above cannot degrade into a no-op.
#[test]
fn mutating_dispatch_without_a_lane_to_provision_still_enforces_occupancy() {
    let (root, plan, occupying_task_id, dispatched_task_id) = plan_with_attempt(None, None);
    let canonical_checkout = root.path().display().to_string();

    let error = preflight_dispatch_placement(
        &plan,
        &dispatched_task_id,
        &canonical_checkout,
        LaneProtection::Enforced,
        false,
    )
    .unwrap_err();

    assert_eq!(error.code, -32011);
    assert_eq!(error.data.unwrap()["occupying_task_id"], occupying_task_id);
}

#[test]
fn distinct_explicit_checkout_does_not_conflict() {
    let root = tempfile::tempdir().unwrap();
    let occupied = root.path().join("lane-a").display().to_string();
    let distinct = root.path().join("lane-b").display().to_string();
    let (_store_root, plan, _, dispatched_task_id) = plan_with_attempt(None, Some(occupied));

    preflight_checkout_occupancy(
        &plan,
        &dispatched_task_id,
        &distinct,
        LaneProtection::Enforced,
    )
    .unwrap();
}
