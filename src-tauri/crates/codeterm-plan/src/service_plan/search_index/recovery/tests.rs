use super::*;
use crate::plan::OrchestratorRef;
use crate::service_plan::{NewTask, PlanStore};

fn create_task(store: &PlanStore, root: &Path, title: &str) -> (String, String) {
    let plan = store
        .create_plan(
            title.into(),
            root.display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: format!("{title} task"),
                ..Default::default()
            },
            1,
        )
        .unwrap();
    (plan.id, task)
}

fn count(conn: &Connection, table: &str, column: &str, entity: &str) -> i64 {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE {column} LIKE ?1"),
        [format!("planunit_{entity}_%")],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn startup_indexes_relational_and_migrated_plans_without_a_mutation() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (relational, relational_task) = create_task(&store, root.path(), "Relational");
    let (migrated, migrated_task) = create_task(&store, root.path(), "Migrated");
    flush_index_queue_for_test(root.path());
    let db = store.database_path_for_test();
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE plans SET document_json = ?1 WHERE id = ?2",
        params![
            serde_json::to_string(&store.get(&migrated).unwrap()).unwrap(),
            migrated
        ],
    )
    .unwrap();
    conn.execute("DELETE FROM plan_tasks WHERE plan_id = ?1", [&migrated])
        .unwrap();
    delete_units(root.path(), "planunit_%").unwrap();
    drop(conn);
    drop(store);

    let reopened = PlanStore::open(db).unwrap();
    flush_index_queue_for_test(root.path());
    let conn = open_index(root.path()).unwrap();
    for (plan, task) in [(relational, relational_task), (migrated, migrated_task)] {
        assert_eq!(reopened.get(&plan).unwrap().tasks[0].id, task);
        assert!(count(&conn, "mem_items", "id", &plan) >= 2);
        let hits = reopened
            .search_plan_index(
                root.path(),
                &PlanIndexQuery {
                    query: reopened.get(&plan).unwrap().tasks[0].name.clone(),
                    kind: Some("task".into()),
                    scope: None,
                    limit: Some(5),
                },
            )
            .unwrap();
        assert!(hits.iter().any(|hit| hit.unit_id == task));
    }
}

#[test]
fn reindex_resolves_project_paths_and_removes_foreign_and_deleted_units() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(other.path());
    let store = PlanStore::new_for_test(root.path());
    let (local, _) = create_task(&store, root.path(), "Local");
    let (foreign, _) = create_task(&store, other.path(), "Foreign");
    let mut local_plan = store.get(&local).unwrap();
    local_plan.canonical_project_path = root.path().join(".").display().to_string();
    store.save(&mut local_plan, 2).unwrap();
    flush_index_queue_for_test(root.path());
    flush_index_queue_for_test(other.path());
    let mut stale = store.get(&foreign).unwrap();
    stale.id = "pl_deleted_fixture".into();
    for plan in [store.get(&foreign).unwrap(), stale] {
        replace_units_once(
            root.path(),
            &format!("planunit_{}_%", plan.id),
            &plan_units(&plan, &project_scope(root.path())),
            false,
        )
        .unwrap();
    }
    let conn = open_index(root.path()).unwrap();
    let vector = stable_id(&foreign, "task", &store.get(&foreign).unwrap().tasks[0].id);
    conn.execute("INSERT INTO mem_vectors(mem_id) VALUES (?1)", [&vector])
        .unwrap();
    conn.execute(
        "INSERT INTO mem_vectors(mem_id) VALUES (?1)",
        [format!("{vector}_chunk_0")],
    )
    .unwrap();
    let result = store.reindex_project_index(root.path()).unwrap();
    flush_index_queue_for_test(root.path());
    assert_eq!(result.plans, 1);
    assert!(count(&conn, "mem_items", "id", &local) > 0);
    for entity in [foreign.as_str(), "pl_deleted_fixture"] {
        for (table, column) in [
            ("mem_items", "id"),
            ("mem_fts", "mem_id"),
            ("mem_vectors", "mem_id"),
        ] {
            assert_eq!(count(&conn, table, column, entity), 0, "{table}: {entity}");
        }
    }
    assert!(
        count(
            &open_index(other.path()).unwrap(),
            "mem_items",
            "id",
            &foreign
        ) > 0
    );
}

#[test]
fn delete_and_retention_purge_plan_and_owned_spec_from_every_index_and_vectors() {
    for retention in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(root.path());
        let (plan_id, _) = create_task(&store, root.path(), "Delete everywhere");
        let plan = store.get(&plan_id).unwrap();
        let mut spec = store
            .create_spec("Owned spec".into(), root.path().display().to_string(), 1)
            .unwrap();
        spec.plan_id = Some(plan_id.clone());
        store.save_spec(&mut spec, 1).unwrap();
        store.mirror_spec_index(&spec).unwrap();
        flush_index_queue_for_test(root.path());
        let foreign_db = port()
            .data_dir()
            .join("plan-indexes")
            .join(project_scope(root.path()))
            .join(INDEX_FILE);
        std::fs::create_dir_all(foreign_db.parent().unwrap()).unwrap();
        let conn = Connection::open(&foreign_db).unwrap();
        let local_conn = open_index(root.path()).unwrap();
        port().init_schema(&conn).unwrap();
        for unit in plan_units(&plan, "foreign")
            .into_iter()
            .chain(spec_units(&spec, "foreign"))
        {
            insert_unit(&conn, &unit).unwrap();
            local_conn
                .execute("INSERT INTO mem_vectors(mem_id) VALUES (?1)", [&unit.id])
                .unwrap();
            conn.execute("INSERT INTO mem_vectors(mem_id) VALUES (?1)", [&unit.id])
                .unwrap();
            conn.execute(
                "INSERT INTO mem_vectors(mem_id) VALUES (?1)",
                [format!("{}_chunk_0", unit.id)],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO mem_vectors(mem_id) VALUES (?1)",
            [format!("planunit_{plan_id}_task_already_missing_chunk_0")],
        )
        .unwrap();
        assert!(count(&conn, "mem_items", "id", &plan_id) > 0);
        if retention {
            assert_eq!(
                store
                    .prune_stale_plans(crate::service_plan::retention::PLAN_RETENTION_MS + 2)
                    .len(),
                1
            );
        } else {
            store.delete(&plan_id).unwrap();
        }
        flush_index_queue_for_test(root.path());
        assert!(store.get(&plan_id).is_none());
        for db in [project_index_path(root.path()), foreign_db] {
            let conn = Connection::open(db).unwrap();
            for id in [&plan_id, &spec.id] {
                for (table, column) in [
                    ("mem_items", "id"),
                    ("mem_fts", "mem_id"),
                    ("mem_vectors", "mem_id"),
                ] {
                    assert_eq!(count(&conn, table, column, id), 0, "{table}: {id}");
                }
            }
        }
    }
}

#[test]
fn vector_cleanup_failure_never_blocks_plan_deletion_and_a_later_purge_removes_units() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, _) = create_task(&store, root.path(), "Deletion refusal");
    flush_index_queue_for_test(root.path());
    let plan = store.get(&plan_id).unwrap();
    let vector = format!("planunit_{plan_id}_plan_{plan_id}");
    let conn = open_index(root.path()).unwrap();
    conn.execute("INSERT INTO mem_vectors(mem_id) VALUES (?1)", [&vector])
        .unwrap();
    conn.execute_batch(
        "CREATE TRIGGER refuse_vector_delete BEFORE DELETE ON mem_vectors
        BEGIN SELECT RAISE(ABORT, 'vector cleanup refused'); END;",
    )
    .unwrap();
    store.delete(&plan_id).unwrap();
    flush_index_queue_for_test(root.path());
    assert!(store.get(&plan_id).is_none());
    assert_eq!(count(&conn, "mem_vectors", "mem_id", &plan_id), 1);
    conn.execute_batch("DROP TRIGGER refuse_vector_delete")
        .unwrap();
    purge_plan_indexes(&plan, &[]).unwrap();
    assert_eq!(count(&conn, "mem_vectors", "mem_id", &plan_id), 0);
}
