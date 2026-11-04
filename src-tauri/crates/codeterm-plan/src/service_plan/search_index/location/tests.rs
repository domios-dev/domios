use super::*;

#[test]
fn moving_search_units_copies_sql_rows_and_vectors_without_body_materialization() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let new = root.path().join("new");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let _store = crate::service_plan::PlanStore::new_for_test(&old);
    let mut plan = Plan::new(
        "move".into(),
        old.display().to_string(),
        crate::plan::OrchestratorRef::default(),
    );
    plan.id = "moving".into();
    mirror_plan(&plan, false).unwrap();
    let source = open_index(&old).unwrap();
    source
        .execute(
            "INSERT INTO mem_vectors(mem_id,embedding) VALUES (?1,?2)",
            params!["planunit_moving_plan_moving", vec![0u8; 4]],
        )
        .unwrap();
    drop(source);
    move_plan_units(&plan.id, &old, &new).unwrap();
    let source = open_index(&old).unwrap();
    assert!(indexed_units_like(&source, "planunit_moving_%")
        .unwrap()
        .is_empty());
    let destination = open_index(&new).unwrap();
    let units = indexed_units_like(&destination, "planunit_moving_%").unwrap();
    assert_eq!(units.len(), 1);
    assert!(units.values().all(|unit| unit.scope == project_scope(&new)));
    assert_eq!(
        destination
            .query_row(
                "SELECT count(*) FROM mem_vectors WHERE mem_id='planunit_moving_plan_moving'",
                [],
                |row| row.get::<_, usize>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn failed_index_move_keeps_source_and_target_atomic() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old");
    let new = root.path().join("new");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let _store = crate::service_plan::PlanStore::new_for_test(&old);
    let mut plan = Plan::new(
        "move".into(),
        old.display().to_string(),
        crate::plan::OrchestratorRef::default(),
    );
    plan.id = "moving".into();
    mirror_plan(&plan, false).unwrap();
    let source = open_index(&old).unwrap();
    source.execute_batch("CREATE TRIGGER reject_move BEFORE DELETE ON mem_items BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    drop(source);
    assert!(move_plan_units(&plan.id, &old, &new).is_err());
    assert_eq!(
        indexed_units_like(&open_index(&old).unwrap(), "planunit_moving_%")
            .unwrap()
            .len(),
        1
    );
    assert!(
        indexed_units_like(&open_index(&new).unwrap(), "planunit_moving_%")
            .unwrap()
            .is_empty()
    );
}
