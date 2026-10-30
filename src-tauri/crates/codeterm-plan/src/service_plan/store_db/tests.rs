use super::*;
use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service_plan::ports::PlanIndexQuery;
use crate::service_plan::{NewTask, PlanStore};

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

    let body = searchable_plan_body(&plan);

    assert!(body.contains("Acceptance marker"));
    assert!(body.contains("https://github.com/rollacode/codeterm-releases/issues/17"));
}

#[test]
fn plan_round_trips_from_one_wal_database() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plans.db");
    let database = PlanDatabase::open(path.clone()).unwrap();
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
    let (plans, specs) = reopened.load_snapshots().unwrap();
    assert_eq!(plans, vec![plan]);
    assert!(specs.is_empty());
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
    let (plans, _) = reopened.load_snapshots().unwrap();
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
