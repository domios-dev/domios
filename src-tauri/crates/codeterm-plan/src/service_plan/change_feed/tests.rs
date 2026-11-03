use super::*;

fn change(plan_id: &str, revision: usize, task_ids: Vec<String>) -> CommittedPlanChange {
    CommittedPlanChange {
        plan_id: plan_id.into(),
        revision: revision.to_string(),
        previous_revision: (revision - 1).to_string(),
        task_ids,
        reset: false,
        deleted: false,
    }
}

fn take(pending: &mut PendingPlanChanges, plan_id: &str) -> CommittedPlanChange {
    match pending.take(plan_id).unwrap() {
        PlanChangeNotice::Committed(change) => change,
        PlanChangeNotice::Resync => panic!("unexpected resync"),
    }
}

#[test]
fn coalesces_commits_without_losing_entity_identity_and_evicts_on_read() {
    let mut pending = PendingPlanChanges::default();
    pending.insert(change("plan", 1, vec!["task-a".into()]));
    pending.insert(change("plan", 2, vec!["task-b".into(), "task-a".into()]));
    let change = take(&mut pending, "plan");
    assert_eq!(change.previous_revision, "0");
    assert_eq!(change.revision, "2");
    assert_eq!(change.task_ids, ["task-a", "task-b"]);
    assert!(pending.take("plan").is_none());
}

#[test]
fn bounds_changed_identity_count_and_bytes_with_scoped_resets() {
    for tasks in [
        (0..=CHANGED_TASK_LIMIT).map(|id| id.to_string()).collect(),
        vec!["x".repeat(CHANGED_TASK_BYTES + 1)],
    ] {
        let mut pending = PendingPlanChanges::default();
        pending.insert(change("plan", 1, tasks));
        let change = take(&mut pending, "plan");
        assert!(change.reset);
        assert!(change.task_ids.is_empty());
        assert!(serde_json::to_vec(&change).unwrap().len() < 256);
    }
}

#[test]
fn pending_plan_overflow_recovers_all_scopes_without_retaining_deleted_plans() {
    let mut pending = PendingPlanChanges::default();
    for id in 0..=PENDING_PLAN_LIMIT {
        pending.insert(change(&id.to_string(), 1, vec![]));
    }
    assert!(pending.plans.is_empty());
    assert!(matches!(pending.take("0"), Some(PlanChangeNotice::Resync)));
    assert!(pending.take("1").is_none());
    let mut deleted = change("deleted", 2, vec![]);
    deleted.deleted = true;
    deleted.reset = true;
    pending.insert(deleted);
    assert!(take(&mut pending, "deleted").deleted);
    assert!(pending.plans.is_empty());
}
