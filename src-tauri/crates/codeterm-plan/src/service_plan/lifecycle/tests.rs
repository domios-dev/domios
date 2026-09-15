use crate::plan::{OrchestratorRef, Plan, PlanMode};
use crate::service_plan::{NewTask, PlanStore};

fn store_with_task() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Lifecycle".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "worker lane".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, p.id.clone(), task_id)
}

#[test]
fn resume_non_paused_plan_preserves_paused_from_mode() {
    let (_dir, store, plan_id, _task_id) = store_with_task();
    store
        .with_plan(&plan_id, 3, |plan| {
            plan.team_id = Some("team-1".into());
            plan.mode = PlanMode::Approved;
            plan.paused_from_mode = Some(PlanMode::Planning);
            Ok(())
        })
        .unwrap();

    let mode = store.resume_plan(&plan_id, 4).unwrap();
    let plan = store.get(&plan_id).unwrap();

    assert_eq!(mode, PlanMode::Approved);
    assert_eq!(plan.mode, PlanMode::Approved);
    assert_eq!(plan.paused_from_mode, Some(PlanMode::Planning));
}
