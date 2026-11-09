use super::*;
use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service_plan::ports::PlanIndexQuery;
use crate::service_plan::{NewTask, PlanStore};

fn fixture_plan(id: &str) -> Plan {
    let project = super::super::test_support::test_data_dir().join(id);
    let mut plan = Plan::new(
        "Fixture plan".into(),
        project.to_string_lossy().into_owned(),
        OrchestratorRef::from_tab("fixture-owner"),
    );
    plan.id = id.into();
    plan.created_at = 11;
    plan.updated_at = 19;
    plan
}

#[test]
fn fixture_batch_reserves_create_only_plans_and_reopens_exact_documents() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let path = store.database.path();
    let mut existing = fixture_plan("existing");
    store.save(&mut existing, 5).unwrap();
    let plans = vec![
        fixture_plan("fixture-first"),
        fixture_plan("fixture-second"),
    ];
    let count = store
        .import_fixture_plans(
            &plans,
            || {
                assert!(plans.iter().all(|plan| store.get(&plan.id).is_none()));
                Ok(plans.len())
            },
            Ok,
        )
        .unwrap();
    assert_eq!(count, 2);
    for plan in &plans {
        assert_eq!(store.get(&plan.id).as_ref(), Some(plan));
    }
    drop(store);
    let reopened = PlanStore::open(path).unwrap();
    assert_eq!(reopened.get("existing"), Some(existing));
    for plan in plans {
        assert_eq!(reopened.get(&plan.id), Some(plan.clone()));
    }
}

#[test]
fn fixture_batch_collision_and_malformed_tail_never_call_desk_commit() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut existing = fixture_plan("existing");
    existing.team_id = Some("existing-team".into());
    store.save(&mut existing, 5).unwrap();
    let mut team_collision = fixture_plan("other-plan");
    team_collision.team_id = existing.team_id.clone();
    let mut malformed = fixture_plan("malformed");
    malformed.title = "x".repeat(257);
    let mut overflow = fixture_plan("overflow");
    overflow.updated_at = u64::MAX;
    for tail in [
        existing.clone(),
        team_collision,
        malformed,
        overflow,
        fixture_plan("fresh"),
    ] {
        let first = fixture_plan("fresh");
        let error = store
            .import_fixture_plans(
                &[first, tail],
                || -> Result<(), String> {
                    panic!("all plan validation must precede the Desk commit")
                },
                Ok,
            )
            .unwrap_err();
        assert!(!error.is_empty());
        assert!(store.get("fresh").is_none());
        assert_eq!(store.get("existing"), Some(existing.clone()));
    }
}

#[test]
fn fixture_batch_failed_desk_stage_creates_no_plans() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let error = store
        .import_fixture_plans(
            &[fixture_plan("new")],
            || -> Result<(), String> { Err("competing desk revision".into()) },
            Ok,
        )
        .unwrap_err();
    assert_eq!(error, "competing desk revision");
    assert!(store.get("new").is_none());
}

#[test]
fn fixture_batch_reservation_orders_a_competing_import_before_any_overwrite() {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let (admitted_tx, admitted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first_store = store.clone();
    let first = std::thread::spawn(move || {
        first_store.import_fixture_plans(
            &[fixture_plan("reserved")],
            || Ok(()),
            |()| {
                admitted_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            },
        )
    });
    admitted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second_store = store.clone();
    let second = std::thread::spawn(move || {
        second_store.import_fixture_plans(
            &[fixture_plan("reserved")],
            || -> Result<(), String> {
                panic!("a competing reserved identity must refuse before Desk commit")
            },
            Ok,
        )
    });
    {
        let mut state = store.mutation_writer.state.lock();
        while state.next_ticket < 2 {
            assert!(!store
                .mutation_writer
                .ready
                .wait_for(&mut state, Duration::from_secs(5))
                .timed_out());
        }
    }
    assert_eq!(store.get("reserved"), Some(fixture_plan("reserved")));
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    assert!(second
        .join()
        .unwrap()
        .unwrap_err()
        .contains("identity collision"));
    assert_eq!(store.get("reserved"), Some(fixture_plan("reserved")));
}

#[test]
fn fixture_batch_plan_io_failure_retains_committed_desk_stage_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let calls = std::cell::Cell::new(0);
    let error = store
        .import_fixture_plans(
            &[fixture_plan("new")],
            || {
                calls.set(calls.get() + 1);
                *store.database.connection.lock() = Connection::open_in_memory().unwrap();
                Ok(())
            },
            Ok,
        )
        .unwrap_err();
    assert_eq!(calls.get(), 1);
    assert!(error.contains("stage=plans desk=committed plans=uncertain"));
    assert!(error.contains("do not replay"));
    assert!(!store.index.read().contains_key("new"));
}

#[test]
fn searchable_plan_body_includes_external_refs() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let plan = store
        .create_plan(
            "External reference database index".into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    store
        .add_task(
            &plan.id,
            NewTask {
                name: "Owning task".into(),
                acceptance_criteria: vec!["Acceptance marker".into()],
                body: "Body marker".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&plan.id).unwrap();
    plan.tasks[0].external_refs = vec![
        crate::service_plan::external_refs::canonicalize_external_ref(
            "rollacode/codeterm-releases#17",
        )
        .unwrap(),
    ];

    store.save(&mut plan, 3).unwrap();
    crate::service_plan::search_index::flush_index_queue_for_test(directory.path());
    let hits = store
        .search_plan_index(
            directory.path(),
            &PlanIndexQuery {
                query: "Acceptance marker".into(),
                kind: Some("task".into()),
                scope: None,
                limit: Some(10),
            },
        )
        .unwrap();
    assert!(hits.iter().any(|hit| hit.body.contains("Acceptance marker")
        && hit
            .body
            .contains("https://github.com/rollacode/codeterm-releases/issues/17")));
}

#[test]
fn task_board_pages_keep_counts_cursor_order_and_compact_history_free_rows() {
    use crate::plan::TaskStatus;

    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let plan = store
        .create_plan(
            "Paged board".into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let paused_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "paused".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let first_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "first".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let second_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "second".into(),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let done_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "done".into(),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    let cancelled_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "cancelled".into(),
                ..Default::default()
            },
            6,
        )
        .unwrap();

    let mut plan_data = store.get(&plan.id).unwrap();
    for task in &mut plan_data.tasks {
        task.created_at = match task.id.as_str() {
            id if id == paused_id => 9,
            _ => 10,
        };
        if task.id == paused_id {
            task.status = TaskStatus::Paused;
        } else if task.id == done_id {
            task.status = TaskStatus::Done;
            task.body = "large detail body".repeat(400);
        } else if task.id == cancelled_id {
            task.status = TaskStatus::Cancelled;
        }
    }
    store.save(&mut plan_data, 7).unwrap();

    let first_page = store.task_board_page(&plan.id, None, 1, None).unwrap();
    assert_eq!(first_page["counts"]["pending"], 3);
    assert_eq!(first_page["counts"]["done"], 1);
    assert_eq!(first_page["counts"]["cancelled"], 1);
    assert_eq!(
        first_page["columns"]["pending"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        first_page["columns"]["pending"]["items"][0]["id"],
        paused_id
    );

    let first_cursor = &first_page["columns"]["pending"]["next_cursor"];
    let first_created = first_cursor["created_at"].as_u64().unwrap();
    let first_position = first_cursor["position"].as_i64().unwrap();
    let first_task_id = first_cursor["task_id"].as_str().unwrap();
    let second_page = store
        .task_board_page(
            &plan.id,
            Some("pending"),
            1,
            Some((first_created, first_position, first_task_id)),
        )
        .unwrap();
    assert_eq!(second_page["column"]["items"][0]["id"], first_id);

    let third_cursor = &second_page["column"]["next_cursor"];
    let third_page = store
        .task_board_page(
            &plan.id,
            Some("pending"),
            1,
            Some((
                third_cursor["created_at"].as_u64().unwrap(),
                third_cursor["position"].as_i64().unwrap(),
                third_cursor["task_id"].as_str().unwrap(),
            )),
        )
        .unwrap();
    assert_eq!(third_page["column"]["items"][0]["id"], second_id);
    assert!(third_page["column"]["next_cursor"].is_null());

    let compact_done = &first_page["columns"]["done"]["items"][0];
    assert!(compact_done.get("body").is_none());
    assert!(compact_done.get("milestones").is_none());
    assert!(compact_done.get("execution_attempts").is_none());
}

#[test]
fn task_board_cursor_pages_use_the_ordered_plan_status_index_without_temp_sort() {
    use rusqlite::Connection;

    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let plan = store
        .create_plan(
            "Indexed task board".into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    for index in 0..3 {
        store
            .add_task(
                &plan.id,
                NewTask {
                    name: format!("indexed-{index}"),
                    ..Default::default()
                },
                index + 2,
            )
            .unwrap();
    }

    let database_path = store.database_path_for_test();
    drop(store);
    let old_database = Connection::open(&database_path).unwrap();
    let registered_index: String = old_database
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='index' AND name='plan_tasks_board_cursor'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    old_database
        .execute_batch("DROP INDEX plan_tasks_board_cursor")
        .unwrap();
    drop(old_database);
    assert!(
        PlanDatabase::open(database_path.clone()).is_err(),
        "an unregistered partial schema must be rejected"
    );
    let fixture_database = Connection::open(&database_path).unwrap();
    fixture_database.execute_batch(&registered_index).unwrap();
    drop(fixture_database);
    let database = PlanDatabase::open(database_path).unwrap();
    let connection = database.connection.lock();
    let index_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'plan_tasks_board_cursor')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        index_exists,
        "the registered schema retains its ordered cursor index"
    );
    let preserved_task_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM plan_tasks WHERE plan_id = ?1",
            [&plan.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        preserved_task_count, 3,
        "reopening preserves existing task rows"
    );
    let explain = |sql: &str, parameters: &[&dyn rusqlite::ToSql]| -> Vec<String> {
        let mut statement = connection.prepare(sql).unwrap();
        statement
            .query_map(parameters, |row| row.get(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let first_page_plan = explain(
        "EXPLAIN QUERY PLAN SELECT task_id FROM plan_tasks
         WHERE plan_id = ?1
           AND CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END = 'pending'
         ORDER BY coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0), position, task_id
         LIMIT ?2",
        &[&plan.id, &2_i64],
    );
    let later_page_plan = explain(
        "EXPLAIN QUERY PLAN SELECT task_id FROM plan_tasks
         WHERE plan_id = ?1
           AND CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END = 'pending'
           AND (coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0), position, task_id) > (?2, ?3, ?4)
         ORDER BY coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0), position, task_id
         LIMIT ?5",
        &[&plan.id, &1_i64, &1_i64, &"cursor", &2_i64],
    );
    for query_plan in [first_page_plan, later_page_plan] {
        let detail = query_plan.join("\n");
        assert!(detail.contains("plan_tasks_board_cursor"), "{detail}");
        assert!(!detail.contains("TEMP B-TREE FOR ORDER BY"), "{detail}");
    }
}

#[test]
fn task_board_cursor_breaks_created_and_position_ties_by_unique_task_id() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let plan = store
        .create_plan(
            "Task cursor ties".into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let first_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "First".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let second_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Second".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks
             SET position = 7, row_json = json_set(row_json, '$.created_at', 9)
             WHERE plan_id = ?1 AND task_id IN (?2, ?3)",
            rusqlite::params![plan.id, first_id, second_id],
        )
        .unwrap();

    let mut expected = [first_id, second_id];
    expected.sort();
    let first_page = store
        .task_board_page(&plan.id, Some("pending"), 1, None)
        .unwrap();
    let first = &first_page["column"]["items"][0];
    assert_eq!(first["id"], expected[0]);
    let cursor = &first_page["column"]["next_cursor"];
    let second_page = store
        .task_board_page(
            &plan.id,
            Some("pending"),
            1,
            Some((
                cursor["created_at"].as_u64().unwrap(),
                cursor["position"].as_i64().unwrap(),
                cursor["task_id"].as_str().unwrap(),
            )),
        )
        .unwrap();
    assert_eq!(second_page["column"]["items"][0]["id"], expected[1]);
    assert!(second_page["column"]["next_cursor"].is_null());
}

#[test]
fn task_detail_reads_one_exact_task_with_its_history_and_small_context() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let plan = store
        .create_plan(
            "Scoped task detail".into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    let selected_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Selected detail".into(),
                body: "Selected task description".into(),
                acceptance_criteria: vec!["Selected criterion".into()],
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let other_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Unselected task".into(),
                body: "Must stay outside the response".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let connection = store.database.connection.lock();
    connection
        .execute(
            "INSERT INTO plan_task_milestones(plan_id, task_id, seq, row_json) VALUES (?1, ?2, 0, ?3)",
            rusqlite::params![
                plan.id,
                selected_id,
                serde_json::json!({ "id": "milestone-1", "kind": "decision", "text": "Selected task diagnostic history", "future_field": true, "ts": 4, "links": [], "loop_id": null, "iteration": null, "canvas": { "x": null, "y": null, "collapsed": false } }).to_string(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO plan_task_attempts(plan_id, task_id, attempt_id, position, content_hash, row_json) VALUES (?1, ?2, 'attempt-1', 1, 0, ?3)",
            rusqlite::params![
                plan.id,
                selected_id,
                serde_json::json!({ "id": "attempt-1", "tab_id": "worker-1", "role": "worker", "report_status": "in_review", "outcome": "review requested", "future_field": true }).to_string(),
            ],
        )
        .unwrap();
    drop(connection);

    let detail = store.task_detail_for_plan(&plan.id, &selected_id).unwrap();
    assert_eq!(detail["plan_id"], plan.id);
    assert_eq!(detail["task_id"], selected_id);
    assert_eq!(detail["task"]["id"], selected_id);
    assert_eq!(detail["task"]["body"], "Selected task description");
    assert_eq!(
        detail["task"]["acceptance_criteria"][0],
        "Selected criterion"
    );
    assert_eq!(
        detail["task"]["milestones"][0]["text"],
        "Selected task diagnostic history"
    );
    assert_eq!(detail["task"]["execution_attempts"][0]["id"], "attempt-1");
    assert_eq!(detail["task"]["milestones"].as_array().unwrap().len(), 1);
    assert_eq!(
        detail["task"]["execution_attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        detail["task"]["milestones"][0]["from"],
        serde_json::json!({
            "tab_id": null, "session_id": null, "title": null,
        })
    );
    assert_eq!(
        detail["task"]["execution_attempts"][0]["executor_tab_id"],
        ""
    );
    for (kind, id) in [
        (
            crate::service_plan::task_history::TaskHistoryKind::Milestones,
            "milestone-1",
        ),
        (
            crate::service_plan::task_history::TaskHistoryKind::Attempts,
            "attempt-1",
        ),
    ] {
        let history = store
            .task_history_page(&plan.id, &selected_id, kind, None, 50)
            .unwrap();
        assert_eq!(history["items"].as_array().unwrap().len(), 1);
        assert_eq!(history["items"][0]["id"], id);
    }

    let chat = store.chat_snapshot(None, None, None).unwrap();
    let chat_task = chat.iter().find(|row| row["id"] == plan.id).unwrap()["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == selected_id)
        .unwrap();
    assert_eq!(chat_task["milestones"].as_array().unwrap().len(), 1);
    assert_eq!(chat_task["execution_attempts"].as_array().unwrap().len(), 1);
    assert_eq!(chat_task["milestones"][0]["id"], "milestone-1");
    assert_eq!(chat_task["execution_attempts"][0]["id"], "attempt-1");
    assert_eq!(
        chat_task["milestones"][0]["from"],
        serde_json::json!({
            "tab_id": null, "session_id": null, "title": null,
        })
    );
    assert_eq!(chat_task["execution_attempts"][0]["executor_tab_id"], "");

    assert_eq!(detail["context"]["plan_id"], plan.id);
    assert!(detail.get("tasks").is_none());
    assert!(detail
        .to_string()
        .find("Must stay outside the response")
        .is_none());

    let other_plan = store
        .create_plan(
            "Different plan".into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            5,
        )
        .unwrap();
    assert!(store
        .task_detail_for_plan(&other_plan.id, &selected_id)
        .is_err());
    assert!(store.task_detail_for_plan(&plan.id, &other_id).is_ok());
}

#[test]
fn plan_round_trips_from_one_wal_database() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let database = PlanDatabase::open(path.clone()).unwrap();
    assert!(path.is_file());
    let published = Connection::open(&path).unwrap();
    let version: i64 = published
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert_eq!(
        published
            .query_row("SELECT COUNT(*) FROM plans", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(published);
    let mut plan = Plan::new(
        "Global plan".into(),
        "D:/project".into(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.team_id = Some("team_one".into());
    plan.updated_at = 7;
    database.persist_plan(&plan, None).unwrap();
    drop(database);

    let reopened = PlanDatabase::open(path).unwrap();
    assert_eq!(reopened.load_plans_for_test(), vec![plan]);
    assert!(reopened.load_specs().unwrap().is_empty());
    let connection = reopened.connection.lock();
    let mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let state: String = connection
        .query_row(
            "SELECT state FROM plan_operations ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "committed");
}

#[test]
fn startup_scan_never_imports_retired_yaml_or_changes_its_bytes() {
    let directory = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(directory.path());
    let legacy = Plan::new(
        "Old plan".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    let legacy_dir =
        crate::service_plan::paths::plan_dir(directory.path(), &legacy.status, &legacy.id).unwrap();
    crate::service_plan::store_v2::write_plan(&legacy_dir, &legacy).unwrap();
    let source = legacy_dir.join("plan.yaml");
    let bytes = std::fs::read(&source).unwrap();
    let store = PlanStore::new_for_test(directory.path());
    store.scan_project(directory.path()).unwrap();
    assert!(store.get(&legacy.id).is_none());
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn deleting_a_plan_removes_the_document_and_search_row_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let plan = store
        .create_plan(
            "Delete searchable plan".into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            11,
        )
        .unwrap();
    let database_path = store.database_path_for_test();
    store.delete(&plan.id).unwrap();
    crate::service_plan::search_index::flush_index_queue_for_test(directory.path());
    drop(store);

    let reopened = PlanStore::open(database_path).unwrap();
    assert!(reopened.get(&plan.id).is_none());
    let hits = reopened
        .search_plan_index(
            directory.path(),
            &PlanIndexQuery {
                query: "Delete searchable".into(),
                kind: None,
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap();
    assert!(hits.is_empty());
}

#[test]
fn plan_store_open_isolates_malformed_and_pane_era_records_without_rewriting() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());

    let mut legacy = Plan::new(
        "Pane-era plan".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("orch-tab"),
    );
    legacy.id = "pl_pane_era".into();
    legacy.planner = Some(PlannerRef::Tab {
        tab_id: "planner-tab".into(),
        session_id: Some("planner-session".into()),
        title: None,
    });
    store.save(&mut legacy, 1).unwrap();
    store
        .add_task(
            &legacy.id,
            NewTask {
                name: "Pane-era attempt".into(),
                handle: Some("pane-era-attempt".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let mut valid = Plan::new(
        "Current plan".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("orch-current"),
    );
    valid.id = "pl_current".into();
    store.save(&mut valid, 3).unwrap();
    let database_path = store.database_path_for_test();

    let mut legacy_document = serde_json::to_value(store.get(&legacy.id).unwrap()).unwrap();
    let planner = legacy_document["planner"].as_object_mut().unwrap();
    let planner_tab = planner.remove("tab_id").unwrap();
    planner.insert("pane_id".into(), planner_tab);
    for key in ["tasks", "sections", "elements", "strides", "heartbeat"] {
        legacy_document.as_object_mut().unwrap().remove(key);
    }

    let connection = Connection::open(&database_path).unwrap();
    connection
        .execute(
            "UPDATE plans SET header_json = ?1 WHERE id = ?2",
            params![legacy_document.to_string(), legacy.id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO plans(id, title, status, team_id, project_path, updated_at, header_json)
             VALUES ('pl_malformed', 'Malformed', 'active', NULL, ?1, 4, '{\"id\":\"pl_malformed\"}')",
            [directory.path().display().to_string()],
        )
        .unwrap();
    drop(connection);
    drop(store);

    let reopened = PlanStore::open(database_path.clone()).unwrap();
    assert_eq!(reopened.get(&valid.id).unwrap().title, "Current plan");
    assert!(reopened.get("pl_malformed").is_none());
    assert!(reopened.get(&legacy.id).is_none());

    let persisted = Connection::open(database_path).unwrap();
    let legacy_source: String = persisted
        .query_row(
            "SELECT header_json FROM plans WHERE id = 'pl_pane_era'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(legacy_source.contains("\"pane_id\""));
    let malformed_count: i64 = persisted
        .query_row(
            "SELECT COUNT(*) FROM plans WHERE id = 'pl_malformed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(malformed_count, 1);
}

#[test]
fn startup_refuses_retired_planner_tags_and_round_trips_current_tags() {
    let raw = serde_json::json!({
        "id": "pl_legacy_tag",
        "title": "Legacy tag",
        "status": "active",
        "owner": {},
        "project_path": "/repo",
        "created_at": 1,
        "updated_at": 2,
        "planner": {"kind": "pane", "tab_id": "tab_x"}
    })
    .to_string();

    assert!(parse_plan_json(&raw).is_err());

    // A document that never carried the legacy tag round-trips unchanged.
    let mut current = Plan::new(
        "Current".into(),
        "/repo".into(),
        OrchestratorRef::from_tab("orch"),
    );
    current.planner = Some(PlannerRef::Tab {
        tab_id: "tab_y".into(),
        session_id: None,
        title: None,
    });
    let current_json = serde_json::to_string(&current).unwrap();
    let reparsed_current = parse_plan_json(&current_json).unwrap();
    assert_eq!(reparsed_current.planner, current.planner);
}

#[test]
fn normalize_pane_id_fields_alone_does_not_fix_legacy_planner_tag_value() {
    // The test protects the distinction: the key normalizer changes field names, never the `kind` tag value.
    let mut value = serde_json::json!({
        "id": "pl_legacy_tag",
        "title": "Legacy tag",
        "status": "active",
        "owner": {},
        "project_path": "/repo",
        "created_at": 1,
        "updated_at": 2,
        "planner": {"kind": "pane", "pane_id": "tab_x"}
    });

    let normalized = codeterm_types::startup_records::normalize_pane_id_fields(&mut value).unwrap();
    assert_eq!(
        normalized, 1,
        "the pane_id -> tab_id key rename still applies"
    );
    assert_eq!(value["planner"]["tab_id"], serde_json::json!("tab_x"));
    assert_eq!(
        value["planner"]["kind"],
        serde_json::json!("pane"),
        "the key normalizer must not have touched the tag value"
    );

    let result: Result<Plan, _> = serde_json::from_value(value);
    assert!(
        result.is_err(),
        "normalize_pane_id_fields alone must not make a legacy-tag document parse"
    );
}

fn stride(id: &str) -> crate::plan::Milestone {
    crate::plan::Milestone {
        id: id.into(),
        from: OrchestratorRef::from_tab("worker"),
        kind: "status_update".into(),
        text: format!("stride {id}"),
        ts: 5,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn plan_with_plan_level_rows() -> Plan {
    let mut plan = Plan::new(
        "Plan-level rows".into(),
        "D:/project".into(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.id = "pl_level".into();
    plan.updated_at = 3;
    plan.team_id = Some("team_rows".into());
    plan.planning_state = crate::plan::PlanningState::Approved;
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "planner-tab".into(),
        session_id: Some("planner-session".into()),
        title: None,
    });
    plan.sections = serde_json::from_value(serde_json::json!([
        {"id": "sec_a", "title": "A", "body": "alpha"},
        {"id": "sec_b", "title": "B", "body": "beta", "subsections": [{"id": "sec_b1", "title": "Nested"}]},
        {"id": "sec_c", "title": "C", "body": "gamma"}
    ]))
    .unwrap();
    plan.elements = serde_json::from_value(serde_json::json!([
        {"id": "el_1", "kind": "note", "content": {"text": "one"}},
        {"id": "el_2", "kind": "link", "links": ["sec_a"]}
    ]))
    .unwrap();
    plan.strides = vec![stride("st0"), stride("st1")];
    plan.heartbeat.current_focus = Some("rows".into());
    plan.heartbeat.active_lanes = vec!["lane/rows".into()];
    plan.tasks = serde_json::from_value(serde_json::json!([{
        "id": "t1", "name": "Rows task", "type": "implement", "status": "pending",
        "execution_attempts": [{
            "id": "a1", "executor_tab_id": "tab-exec",
            "session_id": "session-1", "session_binding_nonce": "nonce-1"
        }]
    }]))
    .unwrap();
    plan
}

#[test]
fn appending_one_stride_writes_one_stride_row_and_no_section_or_element_rows() {
    let directory = tempfile::tempdir().unwrap();
    let database = PlanDatabase::open(directory.path().join("plans.db")).unwrap();
    let mut plan = plan_with_plan_level_rows();
    let first = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        (
            first.sections,
            first.elements,
            first.strides,
            first.heartbeat
        ),
        (3, 2, 2, 1)
    );

    plan.strides.push(stride("st2"));
    plan.updated_at = 4;
    let appended = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        (
            appended.strides,
            appended.sections,
            appended.elements,
            appended.heartbeat,
            appended.tasks,
            appended.milestones,
            appended.attempts,
            appended.deleted
        ),
        (1, 0, 0, 0, 0, 0, 0, 0)
    );
    assert_eq!(database.load_plan(&plan.id).unwrap(), Some(plan.clone()));

    plan.strides[0].text = "edited history".into();
    let rewritten = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!((rewritten.strides, rewritten.deleted), (3, 3));

    plan.sections.swap(0, 2);
    plan.sections[1].body = "changed".into();
    plan.elements.remove(0);
    plan.heartbeat.current_focus = Some("moved on".into());
    let edited = database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        (
            edited.sections,
            edited.elements,
            edited.strides,
            edited.heartbeat,
            edited.deleted
        ),
        (1, 0, 0, 1, 1)
    );
    assert_eq!(database.load_plan(&plan.id).unwrap(), Some(plan));
}

#[test]
fn new_shape_round_trips_every_plan_field() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let database = PlanDatabase::open(path.clone()).unwrap();
    let plan = plan_with_plan_level_rows();
    database.persist_plan(&plan, None).unwrap();
    drop(database);

    let reopened = PlanDatabase::open(path).unwrap();
    let plans = reopened.load_plans_for_test();
    let expected = serde_json::to_value(&plan).unwrap();
    let actual = serde_json::to_value(&plans[0]).unwrap();
    assert_eq!(
        actual.as_object().unwrap().keys().collect::<Vec<_>>(),
        expected.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    assert_eq!(actual, expected);
    assert_eq!(plans, vec![plan]);
    let connection = reopened.connection.lock();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    let header: String = connection
        .query_row("SELECT header_json FROM plans", [], |row| row.get(0))
        .unwrap();
    assert!(header.contains("\"planning_state\":\"approved\""));
}

#[test]
fn generated_query_columns_track_row_json() {
    let directory = tempfile::tempdir().unwrap();
    let database = PlanDatabase::open(directory.path().join("plans.db")).unwrap();
    let mut plan = plan_with_plan_level_rows();
    database.persist_plan(&plan, None).unwrap();
    let task_columns =
        |database: &PlanDatabase| -> (String, Option<String>, Option<String>, Option<String>) {
            database
                .connection
                .lock()
                .query_row(
                    "SELECT status, assigned_tab_id, lane_worktree_path, lane_branch
                 FROM plan_tasks WHERE plan_id = 'pl_level' AND task_id = 't1'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap()
        };
    let attempt_columns =
        |database: &PlanDatabase| -> (String, Option<String>, Option<String>, bool) {
            database
                .connection
                .lock()
                .query_row(
                    "SELECT executor_tab_id, session_id, session_binding_nonce, open
                 FROM plan_task_attempts WHERE plan_id = 'pl_level' AND task_id = 't1'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap()
        };
    assert_eq!(
        task_columns(&database),
        ("pending".into(), None, None, None)
    );
    assert!(attempt_columns(&database).3);

    plan.tasks[0].status = crate::plan::TaskStatus::InProgress;
    plan.tasks[0].assigned_tab_id = Some("tab-worker".into());
    plan.tasks[0].lane.worktree_path = Some("/lanes/t1".into());
    plan.tasks[0].lane.branch = Some("lane/t1".into());
    plan.tasks[0].execution_attempts[0].closed_at = Some(9);
    plan.owner = OrchestratorRef::from_tab("orch-next");
    plan.updated_at = 4;
    database.persist_plan(&plan, None).unwrap();

    assert_eq!(
        task_columns(&database),
        (
            "in_progress".into(),
            Some("tab-worker".into()),
            Some("/lanes/t1".into()),
            Some("lane/t1".into())
        )
    );
    assert_eq!(
        attempt_columns(&database),
        (
            "tab-exec".into(),
            Some("session-1".into()),
            Some("nonce-1".into()),
            false
        )
    );
    let connection = database.connection.lock();
    let owner: String = connection
        .query_row(
            "SELECT owner_tab_id FROM plans WHERE id = 'pl_level'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner, "orch-next");
    let query_plan: String = connection
        .query_row(
            "EXPLAIN QUERY PLAN SELECT task_id FROM plan_tasks WHERE assigned_tab_id = 'tab-worker'",
            [],
            |row| row.get(3),
        )
        .unwrap();
    assert!(
        query_plan.contains("plan_tasks_assigned_tab_id"),
        "{query_plan}"
    );
}
