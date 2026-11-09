use super::*;
use crate::plan::OrchestratorRef;
use rusqlite::Connection;
fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}
#[test]
fn open_builds_the_index_without_parsing_any_plan_row() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut good = Plan::new(
        "Good".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner-tab"),
    );
    store.save(&mut good, 1).unwrap();
    let database_path = store.database_path_for_test();
    drop(store);

    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "INSERT INTO plans(id, title, status, team_id, project_path, updated_at, header_json)
             VALUES ('pl_corrupt', 'Corrupt', 'active', NULL, ?1, 2, '\"this is not a plan\"')",
            [dir.path().display().to_string()],
        )
        .unwrap();
    drop(connection);

    let reopened = PlanStore::open(database_path).unwrap();
    let listed: Vec<String> = reopened.index.read().keys().cloned().collect();
    assert!(
        matches!(reopened.list_index(),Err(PlanError::Io(message)) if message.contains("malformed JSON"))
    );
    assert_eq!(
        sorted(listed),
        sorted(vec![good.id.clone(), "pl_corrupt".into()])
    );
    assert!(
        reopened.plan_loads_for_test().is_empty(),
        "startup isolation and public-list refusal must load no plan"
    );

    assert!(matches!(
        reopened.try_get("pl_corrupt"),
        Err(PlanError::Quarantined { plan_id, .. }) if plan_id == "pl_corrupt"
    ));
    assert!(reopened.get("pl_corrupt").is_none());
    assert_eq!(reopened.get(&good.id).unwrap().title, "Good");
    assert_eq!(
        reopened.plan_loads_for_test(),
        vec![
            "pl_corrupt".to_string(),
            "pl_corrupt".into(),
            good.id.clone()
        ]
    );
}

#[test]
fn fixture_edit_after_a_newer_product_write_is_committed_with_a_monotonic_timestamp() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "fixture".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .update_plan(&plan.id, Some("product update"), None, None, 1000)
        .unwrap();
    store
        .edit_plan_fixture(&plan.id, 6, |plan| {
            plan.team_id = Some("missing-team".into());
            Ok(())
        })
        .unwrap();
    let stored = store.get(&plan.id).unwrap();
    assert_eq!(stored.team_id.as_deref(), Some("missing-team"));
    assert_eq!(stored.title, "product update");
    assert_eq!(stored.updated_at, 1000);
}
