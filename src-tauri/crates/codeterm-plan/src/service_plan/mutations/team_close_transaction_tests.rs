use crate::plan::{OrchestratorRef, PlanStatus};
use crate::service_plan::PlanStore;

#[test]
fn team_detach_transaction_restores_every_bound_plan() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let active = store
        .create_plan_for_team(
            "Active".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
            Some("team_closed".into()),
            vec![],
            1,
        )
        .unwrap();
    let mut archived = store
        .create_plan_for_team(
            "Archived".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
            None,
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
        .detach_and_park_team_plans_transaction("team_closed", 4)
        .unwrap();
    store.rollback_team_plan_detach(transaction).unwrap();

    assert_eq!(store.get(&active.id).unwrap(), before_active);
    assert_eq!(store.get(&archived.id).unwrap(), before_archived);
}
