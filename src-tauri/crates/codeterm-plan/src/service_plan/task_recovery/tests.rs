use super::*;
use crate::plan::{ExecutionIntent, OrchestratorRef, Plan, ProviderAllowance, TaskLaneMetadata};
use crate::service_plan::NewTask;

const GONE_TAB: &str = "worker-tab-gone";

fn mutating_allowance() -> ProviderAllowance {
    ProviderAllowance {
        remaining_pct: 90.0,
        observed_at: 3,
        stale_after_ms: u64::MAX,
        reserve_pct: 10.0,
    }
}

/// A task whose Worker committed dispatch and whose tab is now gone: the exact
/// predicate `verify_task_owner_epoch` can never satisfy from a live caller.
fn tab_gone_task() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "recovery".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "mutate".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = store
        .begin_task_dispatch_with_policy(
            &plan.id,
            &task_id,
            GONE_TAB,
            "Worker",
            TaskLaneMetadata::default(),
            ExecutionIntent::Mutating,
            Some(mutating_allowance()),
            Some("claude"),
            3,
        )
        .unwrap();
    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            GONE_TAB,
            &attempt_id,
            "worker-session",
            "worker-nonce",
            TaskLaneMetadata::default(),
            4,
        )
        .unwrap();
    (dir, store, plan.id, task_id, attempt_id)
}

fn task_of(store: &PlanStore, plan_id: &str, task_id: &str) -> crate::plan::Task {
    store
        .get(plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap()
}

#[test]
fn stale_epoch_is_exactly_what_blocks_the_live_owner_today() {
    let (_dir, store, _plan_id, task_id, _attempt_id) = tab_gone_task();
    // The former owner's identity is still recorded, so no live caller matches.
    assert!(store
        .verify_task_owner_epoch(&task_id, "orch", "orch-session", 1)
        .is_err());
}

#[test]
fn plan_owner_recovers_tab_gone_task_despite_stale_epoch() {
    let (_dir, store, plan_id, task_id, _attempt_id) = tab_gone_task();
    let before = task_of(&store, &plan_id, &task_id);
    assert_eq!(before.interruption.owner_tab_id.as_deref(), Some(GONE_TAB));

    let retirement = store
        .retire_stale_task_owner(
            &plan_id,
            &task_id,
            Some(TaskStatus::Pending),
            |_| false,
            10,
        )
        .expect("a live Plan owner must recover a tab-gone task");

    assert!(retirement.applied);
    assert_eq!(retirement.previous_owner_tab_id.as_deref(), Some(GONE_TAB));
    assert!(retirement.owner_epoch > before.interruption.owner_epoch);

    let after = task_of(&store, &plan_id, &task_id);
    assert_eq!(after.status, TaskStatus::Pending);
    assert!(after.interruption.owner_tab_id.is_none());
    assert!(after.interruption.owner_session_id.is_none());
    assert!(after.assigned_tab_id.is_none());
}

#[test]
fn all_four_recovery_outcomes_are_reachable_from_a_stale_epoch() {
    for outcome in [
        Some(TaskStatus::Cancelled),
        Some(TaskStatus::Pending),
        None,
        None,
    ] {
        let (_dir, store, plan_id, task_id, _attempt_id) = tab_gone_task();
        let retirement = store
            .retire_stale_task_owner(&plan_id, &task_id, outcome.clone(), |_| false, 10)
            .expect("cancel, pend, reassign and respawn all clear the stale owner");
        assert!(retirement.applied);
        let after = task_of(&store, &plan_id, &task_id);
        if let Some(expected) = outcome {
            assert_eq!(after.status, expected);
        }
        // Reassign and respawn both need a task with no owner still holding it.
        assert!(after.interruption.owner_tab_id.is_none());
        assert!(after.lane.tab_id.is_none());
    }
}

#[test]
fn recovery_atomically_retires_the_stale_attempt() {
    let (_dir, store, plan_id, task_id, attempt_id) = tab_gone_task();
    let retirement = store
        .retire_stale_task_owner(&plan_id, &task_id, Some(TaskStatus::Pending), |_| false, 10)
        .unwrap();

    assert_eq!(retirement.retired_attempt_ids, vec![attempt_id.clone()]);
    let after = task_of(&store, &plan_id, &task_id);
    let attempt = after
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    // Retired AND closed in the same write: never observable half-applied.
    assert!(attempt.retired_at.is_some());
    assert!(attempt.closed_at.is_some());
    assert!(after.interruption.owner_tab_id.is_none());
    assert_eq!(after.status_label.as_deref(), Some(OWNER_RETIRED_LABEL));
}

#[test]
fn late_dispatch_ready_for_a_retired_attempt_is_rejected() {
    let (_dir, store, plan_id, task_id, attempt_id) = tab_gone_task();
    store
        .retire_stale_task_owner(&plan_id, &task_id, Some(TaskStatus::Pending), |_| false, 10)
        .unwrap();

    let late = store.finish_task_dispatch(
        &plan_id,
        &task_id,
        GONE_TAB,
        &attempt_id,
        "worker-session",
        "worker-nonce",
        TaskLaneMetadata::default(),
        11,
    );
    assert!(late.is_err(), "a retired attempt must never commit again");

    let after = task_of(&store, &plan_id, &task_id);
    assert_ne!(after.status, TaskStatus::InProgress);
    assert!(after.interruption.owner_tab_id.is_none());
    assert!(!store.worker_startup_attempt_owns(&plan_id, &task_id, GONE_TAB, &attempt_id, None));
}

/// The widening is for the live Plan owner ONLY. Recovery adds no bypass to the
/// owner-epoch predicate itself, so an arbitrary caller is still refused.
#[test]
fn non_owner_cannot_recover_a_tab_gone_task() {
    let (_dir, store, plan_id, task_id, _attempt_id) = tab_gone_task();
    assert!(store
        .verify_task_owner_epoch(&task_id, "some-other-tab", "some-other-session", 1)
        .is_err());

    store
        .retire_stale_task_owner(&plan_id, &task_id, Some(TaskStatus::Pending), |_| false, 10)
        .unwrap();

    // Still refused after recovery: a cleared owner matches nobody either.
    assert!(store
        .verify_task_owner_epoch(&task_id, "some-other-tab", "some-other-session", 2)
        .is_err());
    assert!(store
        .verify_task_owner_epoch(&task_id, GONE_TAB, "worker-session", 1)
        .is_err());
}

#[test]
fn recovery_refuses_while_the_owner_tab_is_still_live() {
    let (_dir, store, plan_id, task_id, _attempt_id) = tab_gone_task();
    let error = store
        .retire_stale_task_owner(&plan_id, &task_id, Some(TaskStatus::Pending), |_| true, 10)
        .unwrap_err();
    assert!(matches!(error, PlanError::Io(_)));
    let after = task_of(&store, &plan_id, &task_id);
    assert_eq!(after.interruption.owner_tab_id.as_deref(), Some(GONE_TAB));
}

#[test]
fn a_second_authority_pass_is_a_no_op_and_never_bumps_the_epoch_twice() {
    let (_dir, store, plan_id, task_id, _attempt_id) = tab_gone_task();
    let first = store
        .retire_stale_task_owner(&plan_id, &task_id, None, |_| false, 10)
        .unwrap();
    let second = store
        .retire_stale_task_owner(&plan_id, &task_id, None, |_| false, 11)
        .unwrap();
    assert!(first.applied);
    assert!(!second.applied);
    assert_eq!(first.owner_epoch, second.owner_epoch);
    assert!(second.retired_attempt_ids.is_empty());
}

#[test]
fn tab_close_retires_the_attempt_so_a_late_commit_cannot_resurrect_it() {
    let (_dir, store, plan_id, task_id, attempt_id) = tab_gone_task();
    assert!(store.flag_tab_gone(&plan_id, &task_id, GONE_TAB, 10).unwrap());

    let after = task_of(&store, &plan_id, &task_id);
    let attempt = after
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert!(attempt.retired_at.is_some());
    assert!(store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            GONE_TAB,
            &attempt_id,
            "worker-session",
            "worker-nonce",
            TaskLaneMetadata::default(),
            11,
        )
        .is_err());
    assert_ne!(
        task_of(&store, &plan_id, &task_id).status,
        TaskStatus::InProgress
    );
}
