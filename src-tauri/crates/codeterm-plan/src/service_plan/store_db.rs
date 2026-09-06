use super::{Plan, PlanError, Spec};
use parking_lot::Mutex;
use rusqlite::{params, Connection, TransactionBehavior};
use std::path::PathBuf;

pub(super) struct PlanDatabase {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl PlanDatabase {
    pub(super) fn open(path: PathBuf) -> Result<Self, PlanError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        let connection = Connection::open(&path).map_err(db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .execute_batch(
                r#"
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS plans (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL,
                    team_id TEXT,
                    project_path TEXT NOT NULL,
                    updated_at INTEGER NOT NULL,
                    document_json TEXT NOT NULL
                );
                CREATE UNIQUE INDEX IF NOT EXISTS one_active_plan_per_team
                    ON plans(team_id)
                    WHERE team_id IS NOT NULL AND status = 'active';
                CREATE INDEX IF NOT EXISTS plans_status_updated
                    ON plans(status, updated_at DESC);

                CREATE TABLE IF NOT EXISTS specs (
                    id TEXT PRIMARY KEY,
                    plan_id TEXT,
                    project_path TEXT NOT NULL,
                    updated_at INTEGER NOT NULL,
                    document_json TEXT NOT NULL,
                    FOREIGN KEY(plan_id) REFERENCES plans(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS plan_operations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    entity_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    state TEXT NOT NULL CHECK(state IN ('queued','running','committed','failed')),
                    submitted_at INTEGER NOT NULL,
                    finished_at INTEGER,
                    error TEXT
                );
                CREATE INDEX IF NOT EXISTS plan_operations_state_id
                    ON plan_operations(state, id);

                CREATE TABLE IF NOT EXISTS legacy_imports (
                    root TEXT PRIMARY KEY,
                    imported_at INTEGER NOT NULL
                );

                CREATE VIRTUAL TABLE IF NOT EXISTS plan_fts USING fts5(
                    plan_id UNINDEXED,
                    title,
                    body
                );
                "#,
            )
            .map_err(db_error)?;
        let _ = connection.execute(
            "ALTER TABLE plans ADD COLUMN title TEXT NOT NULL DEFAULT ''",
            [],
        );
        connection
            .execute(
                "UPDATE plan_operations SET state = 'failed', finished_at = submitted_at, error = 'daemon restarted before commit' WHERE state IN ('queued', 'running')",
                [],
            )
            .map_err(db_error)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    pub(super) fn path(&self) -> PathBuf {
        self.path.clone()
    }

    #[cfg(test)]
    pub(super) fn operation_count_for_test(&self, entity_id: Option<&str>) -> usize {
        let connection = self.connection.lock();
        let count: i64 = match entity_id {
            Some(entity_id) => connection.query_row(
                "SELECT COUNT(*) FROM plan_operations WHERE entity_id = ?1",
                [entity_id],
                |row| row.get(0),
            ),
            None => {
                connection.query_row("SELECT COUNT(*) FROM plan_operations", [], |row| row.get(0))
            }
        }
        .expect("count plan operations");
        count as usize
    }

    pub(super) fn load_snapshots(&self) -> Result<(Vec<Plan>, Vec<Spec>), PlanError> {
        let connection = self.connection.lock();
        let plans = load_plan_documents(&connection)?;
        let specs =
            load_documents::<Spec>(&connection, "spec", "SELECT id, document_json FROM specs")?;
        Ok((plans, specs))
    }

    pub(super) fn load_plan(&self, id: &str) -> Result<Option<Plan>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare("SELECT document_json FROM plans WHERE id = ?1")
            .map_err(db_error)?;
        let mut rows = statement.query([id]).map_err(db_error)?;
        let Some(row) = rows.next().map_err(db_error)? else {
            return Ok(None);
        };
        let document: String = row.get(0).map_err(db_error)?;
        serde_json::from_str(&document)
            .map(Some)
            .map_err(json_error)
    }

    pub(super) fn legacy_imported(&self, root: &str) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM legacy_imports WHERE root = ?1",
                [root],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        Ok(count > 0)
    }

    pub(super) fn mark_legacy_imported(
        &self,
        root: &str,
        imported_at: u64,
    ) -> Result<(), PlanError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "INSERT OR IGNORE INTO legacy_imports(root, imported_at) VALUES (?1, ?2)",
                params![root, imported_at],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(super) fn delete_spec(&self, id: &str, now_ms: u64) -> Result<(), PlanError> {
        let mut connection = self.connection.lock();
        let operation_id = queue_operation(&connection, id, "delete_spec", now_ms)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE plan_operations SET state = 'running' WHERE id = ?1",
                [operation_id],
            )
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM specs WHERE id = ?1", [id])
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE plan_operations SET state = 'committed', finished_at = ?2 WHERE id = ?1",
                params![operation_id, now_ms],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }

    pub(super) fn delete_plan(&self, id: &str, now_ms: u64) -> Result<(), PlanError> {
        let mut connection = self.connection.lock();
        let operation_id = queue_operation(&connection, id, "delete_plan", now_ms)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE plan_operations SET state = 'running' WHERE id = ?1",
                [operation_id],
            )
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM plan_fts WHERE plan_id = ?1", [id])
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM plans WHERE id = ?1", [id])
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE plan_operations SET state = 'committed', finished_at = ?2 WHERE id = ?1",
                params![operation_id, now_ms],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }

    pub(super) fn persist_plan(
        &self,
        plan: &Plan,
        owned_spec: Option<&Spec>,
    ) -> Result<PathBuf, PlanError> {
        let mut connection = self.connection.lock();
        let operation_id = queue_operation(&connection, &plan.id, "persist_plan", plan.updated_at)?;
        let result = (|| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            transaction
                .execute(
                    "UPDATE plan_operations SET state = 'running' WHERE id = ?1",
                    [operation_id],
                )
                .map_err(db_error)?;
            upsert_plan(&transaction, plan)?;
            if let Some(spec) = owned_spec {
                upsert_spec(&transaction, spec)?;
            }
            transaction
                .execute(
                    "UPDATE plan_operations SET state = 'committed', finished_at = ?2 WHERE id = ?1",
                    params![operation_id, plan.updated_at],
                )
                .map_err(db_error)?;
            transaction.commit().map_err(db_error)
        })();
        if let Err(error) = &result {
            let _ = connection.execute(
                "UPDATE plan_operations SET state = 'failed', finished_at = ?2, error = ?3 WHERE id = ?1",
                params![operation_id, plan.updated_at, error.to_string()],
            );
        }
        result.map(|_| self.path())
    }

    pub(super) fn persist_spec(&self, spec: &Spec) -> Result<PathBuf, PlanError> {
        let mut connection = self.connection.lock();
        let operation_id = queue_operation(&connection, &spec.id, "persist_spec", spec.updated_at)?;
        let result = (|| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            transaction
                .execute(
                    "UPDATE plan_operations SET state = 'running' WHERE id = ?1",
                    [operation_id],
                )
                .map_err(db_error)?;
            upsert_spec(&transaction, spec)?;
            transaction
                .execute(
                    "UPDATE plan_operations SET state = 'committed', finished_at = ?2 WHERE id = ?1",
                    params![operation_id, spec.updated_at],
                )
                .map_err(db_error)?;
            transaction.commit().map_err(db_error)
        })();
        if let Err(error) = &result {
            let _ = connection.execute(
                "UPDATE plan_operations SET state = 'failed', finished_at = ?2, error = ?3 WHERE id = ?1",
                params![operation_id, spec.updated_at, error.to_string()],
            );
        }
        result.map(|_| self.path())
    }
}

fn queue_operation(
    connection: &Connection,
    entity_id: &str,
    kind: &str,
    submitted_at: u64,
) -> Result<i64, PlanError> {
    connection
        .execute(
            "INSERT INTO plan_operations(entity_id, kind, state, submitted_at) VALUES (?1, ?2, 'queued', ?3)",
            params![entity_id, kind, submitted_at],
        )
        .map_err(db_error)?;
    Ok(connection.last_insert_rowid())
}

fn upsert_plan(transaction: &rusqlite::Transaction<'_>, plan: &Plan) -> Result<(), PlanError> {
    let document = serde_json::to_string(plan).map_err(json_error)?;
    let status = serde_json::to_value(&plan.status)
        .map_err(json_error)?
        .as_str()
        .unwrap_or("active")
        .to_string();
    transaction
        .execute(
            "INSERT INTO plans(id, title, status, team_id, project_path, updated_at, document_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET title=excluded.title, status=excluded.status,
             team_id=excluded.team_id,
             project_path=excluded.project_path, updated_at=excluded.updated_at,
             document_json=excluded.document_json",
            params![
                plan.id,
                plan.title,
                status,
                plan.team_id,
                plan.project_path,
                plan.updated_at,
                document
            ],
        )
        .map_err(db_error)?;
    transaction
        .execute("DELETE FROM plan_fts WHERE plan_id = ?1", [&plan.id])
        .map_err(db_error)?;
    transaction
        .execute(
            "INSERT INTO plan_fts(plan_id, title, body) VALUES (?1, ?2, ?3)",
            params![plan.id, plan.title, searchable_plan_body(plan)],
        )
        .map_err(db_error)?;
    Ok(())
}

fn upsert_spec(transaction: &rusqlite::Transaction<'_>, spec: &Spec) -> Result<(), PlanError> {
    let document = serde_json::to_string(spec).map_err(json_error)?;
    transaction
        .execute(
            "INSERT INTO specs(id, plan_id, project_path, updated_at, document_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET plan_id=excluded.plan_id,
             project_path=excluded.project_path, updated_at=excluded.updated_at,
             document_json=excluded.document_json",
            params![
                spec.id,
                spec.plan_id,
                spec.project_path,
                spec.updated_at,
                document
            ],
        )
        .map_err(db_error)?;
    Ok(())
}

fn load_plan_documents(connection: &Connection) -> Result<Vec<Plan>, PlanError> {
    let mut statement = connection
        .prepare("SELECT id, document_json FROM plans")
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut plans = Vec::new();
    for row in rows {
        let (record_id, document) = row.map_err(db_error)?;
        match parse_plan_document(&document) {
            Ok((plan, normalized_fields)) => {
                if normalized_fields > 0 {
                    log::warn!(
                        "[plan.store] normalized {} pane-era fields in plan record {}; source left unchanged",
                        normalized_fields,
                        record_id
                    );
                }
                plans.push(plan);
            }
            Err(error) => log::error!(
                "[plan.store] quarantined plan record {} during startup: {}; source left unchanged",
                record_id,
                error
            ),
        }
    }
    Ok(plans)
}

fn parse_plan_document(document: &str) -> Result<(Plan, usize), String> {
    let mut value: serde_json::Value =
        serde_json::from_str(document).map_err(|error| error.to_string())?;
    let normalized_fields = codeterm_types::startup_records::normalize_pane_id_fields(&mut value)?;
    // Key renaming and tag-value migration stay separate because the key normalizer cannot change `planner.kind` values.
    codeterm_types::plan::types::migrate_legacy_planner_tag(&mut value);
    serde_json::from_value(value)
        .map(|plan| (plan, normalized_fields))
        .map_err(|error| error.to_string())
}

fn load_documents<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    record_kind: &str,
    sql: &str,
) -> Result<Vec<T>, PlanError> {
    let mut statement = connection.prepare(sql).map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut documents = Vec::new();
    for row in rows {
        let (record_id, document) = row.map_err(db_error)?;
        match serde_json::from_str(&document) {
            Ok(document) => documents.push(document),
            Err(error) => log::error!(
                "[plan.store] quarantined {} record {} during startup: {}; source left unchanged",
                record_kind,
                record_id,
                error
            ),
        }
    }
    Ok(documents)
}

fn searchable_plan_body(plan: &Plan) -> String {
    let mut body = String::new();
    if let Some(goal) = &plan.goal {
        body.push_str(&goal.vector);
        body.push('\n');
        body.push_str(&goal.vision);
        body.push('\n');
        body.push_str(&goal.constraints.join("\n"));
        body.push('\n');
        body.push_str(&goal.postulates.join("\n"));
        body.push('\n');
    }
    for task in &plan.tasks {
        body.push_str(&task.name);
        body.push('\n');
        body.push_str(&task.acceptance_criteria.join("\n"));
        body.push('\n');
        for external_ref in &task.external_refs {
            body.push_str(&external_ref.canonical_url);
            body.push('\n');
        }
        body.push_str(&task.body);
        body.push('\n');
    }
    body
}

fn db_error(error: rusqlite::Error) -> PlanError {
    PlanError::Io(error.to_string())
}

fn io_error(error: std::io::Error) -> PlanError {
    PlanError::Io(error.to_string())
}

fn json_error(error: serde_json::Error) -> PlanError {
    PlanError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
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
    fn plan_store_restarts_from_database_and_searches_the_same_transaction() {
        let directory = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(directory.path());
        let plan = store
            .create_plan(
                "Restart searchable plan".into(),
                directory.path().display().to_string(),
                OrchestratorRef::from_tab("orch"),
                vec![],
                11,
            )
            .unwrap();
        let path = store.database_path_for_test();
        drop(store);
        crate::service_plan::search_index::flush_index_queue_for_test(directory.path());

        let reopened = PlanStore::open(path).unwrap();
        assert_eq!(reopened.get(&plan.id), Some(plan.clone()));
        let hits = reopened
            .search_plan_index(
                directory.path(),
                &PlanIndexQuery {
                    query: "Restart searchable".into(),
                    kind: None,
                    scope: None,
                    limit: Some(5),
                },
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].plan_id.as_deref(), Some(plan.id.as_str()));
    }

    #[test]
    fn legacy_yaml_is_imported_once_and_never_overwrites_the_database() {
        let directory = tempfile::tempdir().unwrap();
        crate::service_plan::paths::mark_test_project_root(directory.path());
        let mut legacy_plan = Plan::new(
            "Imported title".into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        legacy_plan.updated_at = 10;
        let legacy_dir = crate::service_plan::paths::plan_dir(
            directory.path(),
            &legacy_plan.status,
            &legacy_plan.id,
        )
        .unwrap();
        crate::service_plan::store_v2::write_plan(&legacy_dir, &legacy_plan).unwrap();

        let store = PlanStore::new_for_test(directory.path());
        store.scan_project(directory.path()).unwrap();
        assert_eq!(store.get(&legacy_plan.id).unwrap().title, "Imported title");
        let database_path = store.database_path_for_test();
        drop(store);

        legacy_plan.title = "Stale YAML must not win".into();
        legacy_plan.updated_at = 99;
        crate::service_plan::store_v2::write_plan(&legacy_dir, &legacy_plan).unwrap();

        let reopened = PlanStore::open(database_path).unwrap();
        let operations_before_rescan = reopened.operation_count_for_test(None);
        reopened.scan_project(directory.path()).unwrap();
        assert_eq!(
            reopened.get(&legacy_plan.id).unwrap().title,
            "Imported title"
        );
        assert_eq!(
            reopened.operation_count_for_test(None),
            operations_before_rescan
        );
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
    fn plan_store_open_isolates_malformed_records_and_normalizes_pane_era_fields() {
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
        legacy_document["tasks"][0]["execution_attempts"] = serde_json::json!([{
            "id": "attempt-pane-era",
            "executor_pane_id": "worker-tab",
            "parent_pane_id": "parent-tab",
            "orchestrator_pane_id": "orch-tab"
        }]);

        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute(
                "UPDATE plans SET document_json = ?1 WHERE id = ?2",
                params![legacy_document.to_string(), legacy.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO plans(id, title, status, team_id, project_path, updated_at, document_json)
                 VALUES ('pl_malformed', 'Malformed', 'active', NULL, ?1, 4, '{\"id\":\"pl_malformed\"}')",
                [directory.path().display().to_string()],
            )
            .unwrap();
        drop(connection);
        drop(store);

        let reopened = PlanStore::open(database_path.clone()).unwrap();
        assert_eq!(reopened.get(&valid.id).unwrap().title, "Current plan");
        assert!(reopened.get("pl_malformed").is_none());
        let normalized = reopened.get(&legacy.id).unwrap();
        assert!(matches!(
            normalized.planner,
            Some(PlannerRef::Tab { ref tab_id, .. }) if tab_id == "planner-tab"
        ));
        let attempt = &normalized.tasks[0].execution_attempts[0];
        assert_eq!(attempt.executor_tab_id, "worker-tab");
        assert_eq!(attempt.parent_tab_id.as_deref(), Some("parent-tab"));
        assert_eq!(attempt.orchestrator_tab_id.as_deref(), Some("orch-tab"));

        let persisted = Connection::open(database_path).unwrap();
        let legacy_source: String = persisted
            .query_row(
                "SELECT document_json FROM plans WHERE id = 'pl_pane_era'",
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
    fn legacy_planner_tag_value_migrates_without_quarantine() {
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

        let (plan, _) =
            parse_plan_document(&raw).expect("a legacy-tag document must load, not quarantine");
        assert!(matches!(
            plan.planner,
            Some(PlannerRef::Tab { ref tab_id, .. }) if tab_id == "tab_x"
        ));

        // Idempotent: re-parsing the already-migrated document yields the same value.
        let migrated_json = serde_json::to_string(&plan).unwrap();
        let (reparsed, _) = parse_plan_document(&migrated_json).unwrap();
        assert_eq!(reparsed.planner, plan.planner);

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
        let (reparsed_current, normalized) = parse_plan_document(&current_json).unwrap();
        assert_eq!(reparsed_current.planner, current.planner);
        assert_eq!(normalized, 0);
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

        let normalized =
            codeterm_types::startup_records::normalize_pane_id_fields(&mut value).unwrap();
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
}
