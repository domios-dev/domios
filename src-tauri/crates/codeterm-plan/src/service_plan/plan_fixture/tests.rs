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
    let listed: Vec<String> = reopened
        .list_index()
        .unwrap()
        .into_iter()
        .map(|entry| entry.plan_id)
        .collect();
    assert_eq!(
        sorted(listed),
        sorted(vec![good.id.clone(), "pl_corrupt".into()])
    );
    assert!(
        reopened.plan_loads_for_test().is_empty(),
        "open and list_index must load no plan"
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
