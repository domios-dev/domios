use crate::plan::{OrchestratorRef, PlanStatus, PlannerRef};
use crate::service_plan::{NewTask, PlanStore};

#[test]
fn team_archive_transaction_restores_every_bound_plan() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let active = store
        .create_plan_for_team(
            "Active".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
            "team_closed".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut archived = store
        .create_plan_for_team(
            "Archived".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
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
fn archiving_a_large_team_plan_is_one_database_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan_for_team(
            "Large plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
            "team_closed".into(),
            vec![],
            1,
        )
        .unwrap();
    store
        .set_planner_ref(
            &plan.id,
            Some(PlannerRef::Pane {
                pane_id: "planner".into(),
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
    let archived = store.get(&plan.id).unwrap();
    assert!(archived.team_id.is_none());
    assert_eq!(archived.status, PlanStatus::Archived);
    assert_eq!(archived.owner, OrchestratorRef::default());
    assert_eq!(archived.planner, None);
    assert_eq!(
        archived
            .owner_history
            .last()
            .unwrap()
            .from
            .pane_id
            .as_deref(),
        Some("orch")
    );
    assert_eq!(archived.tasks.len(), 64);
}
