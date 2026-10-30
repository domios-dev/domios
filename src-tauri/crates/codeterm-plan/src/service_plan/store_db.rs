use super::store_rows;
use super::store_sync::{self, RowWrites};
use super::{Plan, PlanError, Spec};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
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

                CREATE VIRTUAL TABLE IF NOT EXISTS plan_fts USING fts5(
                    plan_id UNINDEXED,
                    title,
                    body
                );
                "#,
            )
            .map_err(db_error)?;
        connection
            .execute_batch(store_sync::TASK_SCHEMA)
            .map_err(db_error)?;
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
        let Some(document) = store_sync::plan_document(&connection, id).map_err(db_error)? else {
            return Ok(None);
        };
        let value =
            store_sync::load_plan_value(&connection, id, &document).map_err(PlanError::Io)?;
        serde_json::from_value(value).map(Some).map_err(json_error)
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

    #[cfg(test)]
    pub(super) fn persist_plan(
        &self,
        plan: &Plan,
        owned_spec: Option<&Spec>,
    ) -> Result<PathBuf, PlanError> {
        self.persist_plan_rows(plan, owned_spec)
            .map(|_| self.path())
    }

    pub(super) fn persist_plan_rows(
        &self,
        plan: &Plan,
        owned_spec: Option<&Spec>,
    ) -> Result<RowWrites, PlanError> {
        let rows = store_rows::split_plan(serde_json::to_value(plan).map_err(json_error)?)
            .map_err(PlanError::Io)?;
        store_rows::validate_row_sizes(&rows).map_err(PlanError::InvalidPlanText)?;
        let mut connection = self.connection.lock();
        let operation_id = queue_operation(&connection, &plan.id, "persist_plan", plan.updated_at)?;
        let result = (|| -> Result<RowWrites, PlanError> {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            transaction
                .execute(
                    "UPDATE plan_operations SET state = 'running' WHERE id = ?1",
                    [operation_id],
                )
                .map_err(db_error)?;
            let writes = upsert_plan(&transaction, plan, &rows)?;
            if let Some(spec) = owned_spec {
                upsert_spec(&transaction, spec)?;
            }
            transaction
                .execute(
                    "UPDATE plan_operations SET state = 'committed', finished_at = ?2 WHERE id = ?1",
                    params![operation_id, plan.updated_at],
                )
                .map_err(db_error)?;
            transaction.commit().map_err(db_error)?;
            Ok(writes)
        })();
        if let Err(error) = &result {
            let _ = connection.execute(
                "UPDATE plan_operations SET state = 'failed', finished_at = ?2, error = ?3 WHERE id = ?1",
                params![operation_id, plan.updated_at, error.to_string()],
            );
        }
        result
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

fn upsert_plan(
    transaction: &rusqlite::Transaction<'_>,
    plan: &Plan,
    rows: &store_rows::PlanRows,
) -> Result<RowWrites, PlanError> {
    let status = serde_json::to_value(&plan.status)
        .map_err(json_error)?
        .as_str()
        .unwrap_or("active")
        .to_string();
    let previous_header: Option<(String, String, String, String)> = transaction
        .query_row(
            "SELECT title, status, project_path,
                    coalesce(json_extract(document_json, '$.canonical_project_path'), '')
             FROM plans WHERE id = ?1",
            [&plan.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(db_error)?;
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
                rows.plan
            ],
        )
        .map_err(db_error)?;
    let mut writes =
        store_sync::sync_task_rows(transaction, &plan.id, &rows.tasks).map_err(db_error)?;
    if let Some((title, previous_status, project_path, canonical_project_path)) = previous_header {
        if project_path != plan.project_path
            || canonical_project_path != plan.canonical_project_path
        {
            writes.previous_project_paths = Some((project_path, canonical_project_path));
        }
        writes.header_identity_changed = title != plan.title
            || previous_status != status
            || writes.previous_project_paths.is_some();
    }
    writes.header_identity_changed |= rows.kept_whole.is_some();
    transaction
        .execute("DELETE FROM plan_fts WHERE plan_id = ?1", [&plan.id])
        .map_err(db_error)?;
    transaction
        .execute(
            "INSERT INTO plan_fts(plan_id, title, body) VALUES (?1, ?2, ?3)",
            params![plan.id, plan.title, searchable_plan_body(plan)],
        )
        .map_err(db_error)?;
    Ok(writes)
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
    let documents = rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
    drop(statement);
    for (record_id, document) in documents {
        let joined = store_sync::load_plan_value(connection, &record_id, &document);
        match joined.and_then(parse_plan_value) {
            Ok(plan) => plans.push(plan),
            Err(error) => log::error!(
                "[plan.store] quarantined plan record {} during startup: {}; source left unchanged",
                record_id,
                error
            ),
        }
    }
    Ok(plans)
}

#[cfg(test)]
fn parse_plan_document(document: &str) -> Result<Plan, String> {
    parse_plan_value(serde_json::from_str(document).map_err(|error| error.to_string())?)
}

pub(super) fn parse_plan_value(value: serde_json::Value) -> Result<Plan, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
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
mod schema_tests;
#[cfg(test)]
mod snapshot_check;
mod startup_queries;
#[cfg(test)]
mod tests;
