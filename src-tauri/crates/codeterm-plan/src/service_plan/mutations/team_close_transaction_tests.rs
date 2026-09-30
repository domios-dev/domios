use crate::plan::reconcile::TEAM_CLOSED_LABEL;
use crate::plan::{OrchestratorRef, PlanStatus, PlannerRef, TaskExecutionAttempt, TaskStatus};
use crate::service_plan::{NewTask, PlanStore};

#[test]
fn team_archive_transaction_restores_every_bound_plan() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let active = store
        .create_plan_for_team(
            "Active".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team_closed".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut archived = store
        .create_plan_for_team(
            "Archived".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team_archived".into(),
            vec![],
            2,
        )
        .unwrap();
    archived.team_id = Some("team_closed".into());
    archived.status = PlanStatus::Archived;
    store.save(&mut archived, 3).unwrap();
    let before_active = store.get(&active.id).unwrap();
    let before_archived = store.get(&archived.id).unwrap();

    let transaction = store
        .archive_and_detach_team_plans_transaction("team_closed", 4)
        .unwrap();
    store.rollback_team_plan_archive(transaction).unwrap();

    assert_eq!(store.get(&active.id).unwrap(), before_active);
    assert_eq!(store.get(&archived.id).unwrap(), before_archived);
}

#[test]
fn detaching_a_large_unfinished_team_plan_is_one_database_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan_for_team(
            "Large plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team_closed".into(),
            vec![],
            1,
        )
        .unwrap();
    store
        .set_planner_ref(
            &plan.id,
            Some(PlannerRef::Tab {
                tab_id: "planner".into(),
                session_id: None,
                title: None,
            }),
            66,
        )
        .unwrap();
    for number in 0..64 {
        store
            .add_task(
                &plan.id,
                NewTask {
                    name: format!("Task {number}"),
                    kind: "fix".into(),
                    body: format!("large plan task body {number}"),
                    ..Default::default()
                },
                number + 2,
            )
            .unwrap();
    }
    let before = store.operation_count_for_test(Some(&plan.id));

    store
        .archive_and_detach_team_plans_transaction("team_closed", 100)
        .unwrap();

    assert_eq!(store.operation_count_for_test(Some(&plan.id)), before + 1);
    let detached = store.get(&plan.id).unwrap();
    assert!(detached.team_id.is_none());
    assert_eq!(detached.status, PlanStatus::Active);
    assert_eq!(
        detached
            .orphaned
            .as_ref()
            .map(|orphan| orphan.unfinished_task_ids.len()),
        Some(64)
    );
    assert_eq!(detached.owner, OrchestratorRef::default());
    assert_eq!(detached.planner, None);
    assert_eq!(
        detached
            .owner_history
            .last()
            .unwrap()
            .from
            .tab_id
            .as_deref(),
        Some("orch")
    );
    assert_eq!(detached.tasks.len(), 64);
}

#[test]
fn team_close_force_keeps_plan_active_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan_for_team(
            "Busy plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team_busy".into(),
            vec![],
            1,
        )
        .unwrap();
    let busy = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Busy task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let finished = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Finished task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 4, |plan| {
            for task in &mut plan.tasks {
                if task.id == busy {
                    task.status = TaskStatus::InProgress;
                    task.assigned_tab_id = Some("worker".into());
                    task.lane.tab_id = Some("worker".into());
                    task.execution_attempts.push(TaskExecutionAttempt {
                        id: "attempt_busy".into(),
                        executor_tab_id: "worker".into(),
                        ..Default::default()
                    });
                } else if task.id == finished {
                    task.status = TaskStatus::Done;
                }
            }
            Ok(())
        })
        .unwrap();
    let members = vec!["orch".to_string(), "worker".to_string()];

    let transaction = store
        .close_team_task_attempts_transaction("team_busy", &members, 5)
        .unwrap();

    assert_eq!(transaction.changed_plan_ids(), [plan.id.clone()]);
    let flagged = store.get(&plan.id).unwrap();
    assert_eq!(flagged.status, PlanStatus::Active);
    assert!(flagged.is_orphaned_active());
    let orphan = flagged.orphaned.clone().unwrap();
    assert_eq!(orphan.team_id, "team_busy");
    assert_eq!(orphan.unfinished_task_ids, vec![busy.clone()]);
    let task = flagged.tasks.iter().find(|task| task.id == busy).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.status_label.as_deref(), Some(TEAM_CLOSED_LABEL));
    assert_eq!(task.assigned_tab_id, None);
    assert!(task.execution_attempts[0].closed_at.is_some());
    let entry = store
        .list_index()
        .into_iter()
        .find(|entry| entry.plan_id == plan.id)
        .unwrap();
    assert_eq!(entry.status, PlanStatus::Active);
    assert_eq!(entry.orphaned.as_deref(), Some(orphan.reason.as_str()));

    assert!(store
        .archive_active_plans_without_live_team(&std::collections::BTreeSet::new(), 6)
        .unwrap()
        .is_empty());
    assert_eq!(store.get(&plan.id).unwrap().status, PlanStatus::Active);

    let adopted = store
        .adopt_plan_for_team(&plan.id, "team_next", OrchestratorRef::from_tab("next"), 7)
        .unwrap();
    assert_eq!(adopted.team_id.as_deref(), Some("team_next"));
    assert_eq!(adopted.orphaned, None);
}
