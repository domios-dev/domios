use super::*;
#[test]
fn close_metadata_restore_uses_selected_rows_and_rechecks_owner() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "restore".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.team_id = Some("team".into());
    store.save(&mut plan, 1).unwrap();
    let saved = store
        .close_restore_projection(&["team".into()])
        .unwrap()
        .pop()
        .unwrap();
    store.archive(&plan.id, 2).unwrap();
    crate::service_plan::load_audit::start(&store);
    store.restore_close_plan_metadata(&saved, 3).unwrap();
    assert!(store.plan_loads_for_test().is_empty());
    let restored = store.get(&plan.id).unwrap();
    assert_eq!(restored.status, saved.status);
    assert_eq!(restored.owner, saved.owner);
    assert_eq!(restored.team_id, saved.team_id);
    store
        .transfer(&plan.id, OrchestratorRef::from_tab("other"), "new owner", 4)
        .unwrap();
    assert!(store.restore_close_plan_metadata(&saved, 5).is_err());
    assert_eq!(
        store.get(&plan.id).unwrap().owner.tab_id.as_deref(),
        Some("other")
    );
}
