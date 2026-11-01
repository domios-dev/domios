use crate::plan::OrchestratorRef;
use crate::service_plan::PlanStore;

#[test]
fn schema_template_isolates_fixtures_and_preserves_existing_databases() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(first.path());
    let plan = store
        .create_plan(
            "Persisted fixture".into(),
            first.path().display().to_string(),
            OrchestratorRef::from_tab("owner"),
            vec![],
            1,
        )
        .unwrap();
    drop(store);

    let second_database = second.path().join(".domios/plans.db");
    std::thread::scope(|threads| {
        for _ in 0..4 {
            threads.spawn(|| super::seed_plan_database(&second_database));
        }
    });
    let independent = PlanStore::new_for_test(second.path());
    assert!(independent.get(&plan.id).is_none());
    let reopened = PlanStore::new_for_test(first.path());
    assert_eq!(reopened.get(&plan.id).unwrap(), plan);
    let database = first.path().join(".domios/plans.db");
    let bytes = std::fs::read(&database).unwrap();
    std::thread::scope(|threads| {
        for _ in 0..4 {
            threads.spawn(|| super::seed_plan_database(&database));
        }
    });
    assert_eq!(std::fs::read(database).unwrap(), bytes);
}

#[test]
fn seeded_fixture_database_has_the_current_row_schema() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join(".domios/plans.db");
    super::seed_plan_database(&database);

    let connection = rusqlite::Connection::open(&database).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let header_columns: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('plans') WHERE name = 'header_json'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(header_columns, 1);
    let backups = std::fs::read_dir(directory.path().join(".domios"))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".pre-")
        })
        .count();
    assert_eq!(backups, 0);
}
