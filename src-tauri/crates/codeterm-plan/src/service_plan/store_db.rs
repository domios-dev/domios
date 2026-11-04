use super::store_rows;
use super::store_sync::{self, RowWrites, StoredStrides};
use super::{Plan, PlanError, Spec};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::PathBuf;

pub(super) const SCHEMA_VERSION: i64 = migrations::SCHEMA_VERSION;
pub(in crate::service_plan) mod migrations;

#[cfg(test)]
const PLAN_SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS plans (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        team_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        stride_count INTEGER NOT NULL DEFAULT 0,
        stride_digest INTEGER NOT NULL DEFAULT 0,
        header_json TEXT NOT NULL,
        owner_tab_id TEXT GENERATED ALWAYS AS (json_extract(header_json, '$.owner.tab_id')) VIRTUAL
    );
    CREATE UNIQUE INDEX IF NOT EXISTS one_active_plan_per_team
        ON plans(team_id)
        WHERE team_id IS NOT NULL AND status = 'active';
    CREATE INDEX IF NOT EXISTS plans_status_updated
        ON plans(status, updated_at DESC);
    CREATE INDEX IF NOT EXISTS plans_owner_tab_id
        ON plans(owner_tab_id) WHERE owner_tab_id IS NOT NULL;

    CREATE TABLE IF NOT EXISTS specs (
        id TEXT PRIMARY KEY,
        plan_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        row_json TEXT NOT NULL,
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
    CREATE INDEX IF NOT EXISTS plan_operations_entity_id
        ON plan_operations(entity_id);
    CREATE INDEX IF NOT EXISTS plan_operations_finished_id
        ON plan_operations(finished_at DESC, id DESC)
        WHERE state IN ('committed', 'failed') AND finished_at IS NOT NULL;

"#;

#[cfg(test)]
const TAB_INDEX_SCHEMA: &str = r#"
    CREATE INDEX IF NOT EXISTS plans_planner_tab_id
        ON plans(json_extract(header_json, '$.planner.tab_id'), id)
        WHERE json_extract(header_json, '$.planner.tab_id') IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_tasks_lane_tab_id
        ON plan_tasks(json_extract(row_json, '$.lane.tab_id'), plan_id)
        WHERE json_extract(row_json, '$.lane.tab_id') IS NOT NULL;
CREATE INDEX IF NOT EXISTS plan_tasks_event_order ON plan_tasks(
    plan_id, coalesce(json_extract(row_json,'$.created_at'),0) DESC, position DESC,
    task_id, json_extract(row_json,'$.name'));
CREATE INDEX IF NOT EXISTS plan_strides_event_order ON plan_strides(
    plan_id, coalesce(json_extract(row_json,'$.ts'),0) DESC, seq DESC);
CREATE INDEX IF NOT EXISTS plan_milestones_event_order ON plan_task_milestones(
    plan_id, coalesce(json_extract(row_json,'$.ts'),0) DESC, task_id, seq DESC);
CREATE INDEX IF NOT EXISTS plan_attempts_event_author ON plan_task_attempts(
    plan_id, task_id, executor_tab_id, position DESC, json_extract(row_json,'$.executor_title'))
    WHERE json_extract(row_json,'$.executor_title') IS NOT NULL;
"#;

#[cfg(test)]
pub(super) fn create_schema(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(PLAN_SCHEMA)?;
    connection.execute_batch(store_sync::TASK_SCHEMA)?;
    connection.execute_batch(store_sync::PLAN_LEVEL_SCHEMA)?;
    connection.execute_batch(TAB_INDEX_SCHEMA)?;
    connection.pragma_update(None, "application_id", 0x44504c4e_i64)
}

pub(super) struct PlanDatabase {
    path: PathBuf,
    connection: Mutex<Connection>,
    pub(super) pending_changes: Mutex<super::change_feed::PendingPlanChanges>,
}

impl PlanDatabase {
    pub(super) fn open(path: PathBuf) -> Result<Self, PlanError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut connection = if path.exists() {
            Connection::open(&path)
        } else {
            Connection::open_in_memory()
        }
        .map_err(db_error)?;
        migrations::initialize(&mut connection, &path)?;
        status_queries::register_functions(&connection)?;
        state_event_window::register_functions(&connection)?;
        heartbeat_queries::register_functions(&connection)?;
        reference_resolver_queries::register_functions(&connection)?;
        task_key_queries::register_functions(&connection)?;
        curator_notice_queries::register_functions(&connection)?;
        auxiliary_queries::register_functions(&connection)?;
        worker_read_queries::register_functions(&connection)?;
        list_window_queries::register_functions(&connection)?;
        task_authority_queries::register_functions(&connection)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        connection
            .execute_batch(
                "PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;",
            )
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
            pending_changes: Mutex::new(super::change_feed::PendingPlanChanges::default()),
        })
    }

    pub(super) fn path(&self) -> PathBuf {
        self.path.clone()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn with_connection_held_for_test<T>(&self, work: impl FnOnce() -> T) -> T {
        let _connection = self.connection.lock();
        work()
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

    pub(super) fn load_specs(&self) -> Result<Vec<Spec>, PlanError> {
        let connection = self.connection.lock();
        load_documents::<Spec>(&connection, "spec", "SELECT id, row_json FROM specs")
    }

    /// Every parseable plan, for test oracles only; the store loads plans one at a time.
    #[cfg(test)]
    pub(super) fn load_plans_for_test(&self) -> Vec<Plan> {
        let ids: Vec<String> = {
            let connection = self.connection.lock();
            let mut statement = connection
                .prepare("SELECT id FROM plans ORDER BY id")
                .expect("prepare plan ids");
            statement
                .query_map([], |row| row.get(0))
                .expect("query plan ids")
                .collect::<Result<_, _>>()
                .expect("read plan ids")
        };
        ids.iter()
            .filter_map(|id| self.load_plan(id).ok().flatten())
            .collect()
    }

    /// `Ok(None)` when no row exists; rows that do not parse are `PlanError::Quarantined`.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn load_plan(&self, id: &str) -> Result<Option<Plan>, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let header: Option<String> = transaction
            .query_row("SELECT header_json FROM plans WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_error)?;
        let Some(header) = header else {
            return Ok(None);
        };
        let result = store_sync::load_plan_rows(&transaction, id, &header)
            .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
            .map(Some)
            .map_err(|reason| PlanError::Quarantined {
                plan_id: id.to_string(),
                reason,
            });
        transaction.commit().map_err(db_error)?;
        result
    }

    #[cfg(test)]
    pub(super) fn delete_plan(&self, id: &str, now_ms: u64) -> Result<(), PlanError> {
        let mut connection = self.connection.lock();
        let operation_id = queue_operation(&connection, id, "delete_plan", now_ms)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let previous_revision = change_feed::revision(&transaction, id)?;
        let revision = change_feed::next_revision(previous_revision, operation_id)?;
        transaction
            .execute(
                "UPDATE plan_operations SET state = 'running' WHERE id = ?1",
                [operation_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE plan_operations SET state = 'committed', finished_at = ?2 WHERE id = ?1",
                params![operation_id, now_ms],
            )
            .map_err(db_error)?;
        operation_retention::prune_deleted_plan(&transaction, id)?;
        transaction
            .execute("DELETE FROM plans WHERE id = ?1", [id])
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        self.pending_changes.lock().insert(change_feed::change(
            id,
            revision,
            previous_revision,
            Vec::new(),
            true,
            true,
        ));
        Ok(())
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

    #[cfg(any(test, feature = "test-support"))]
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
            let previous_revision = change_feed::revision(&transaction, &plan.id)?;
            let revision = change_feed::next_revision(previous_revision, operation_id)?;
            let writes = upsert_plan(&transaction, plan, &rows, Some(revision))?;
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
            self.pending_changes.lock().insert(change_feed::change(
                &plan.id,
                revision,
                previous_revision,
                Vec::new(),
                true,
                false,
            ));
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

    pub(super) fn prepare_fixture_plans(
        plans: &[Plan],
    ) -> Result<Vec<store_rows::PlanRows>, PlanError> {
        plans
            .iter()
            .map(|plan| {
                i64::try_from(plan.updated_at).map_err(|_| {
                    PlanError::Io("Fixture plan timestamp exceeds SQLite integer range".into())
                })?;
                for task in &plan.tasks {
                    i64::try_from(task.updated_at).map_err(|_| {
                        PlanError::Io("Fixture task timestamp exceeds SQLite integer range".into())
                    })?;
                }
                let rows = store_rows::split_plan(serde_json::to_value(plan).map_err(json_error)?)
                    .map_err(PlanError::Io)?;
                store_rows::validate_row_sizes(&rows).map_err(PlanError::InvalidPlanText)?;
                Ok(rows)
            })
            .collect()
    }

    pub(super) fn persist_fixture_plans(
        &self,
        plans: &[Plan],
        rows: &[store_rows::PlanRows],
    ) -> Result<(), PlanError> {
        if plans.len() != rows.len() {
            return Err(PlanError::Io("Fixture plan row count mismatch".into()));
        }
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        for (plan, rows) in plans.iter().zip(rows) {
            upsert_plan(&transaction, plan, rows, None)?;
        }
        transaction.commit().map_err(db_error)
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
    revision: Option<i64>,
) -> Result<RowWrites, PlanError> {
    let writes = write_plan_rows_at_revision(transaction, plan, rows, revision)?;
    Ok(writes)
}

/// Writes a plan's header, task and plan-level rows, touching only the rows that changed.
pub(super) fn write_plan_rows(
    transaction: &rusqlite::Transaction<'_>,
    plan: &Plan,
    rows: &store_rows::PlanRows,
) -> Result<RowWrites, PlanError> {
    write_plan_rows_at_revision(transaction, plan, rows, None)
}

fn write_plan_rows_at_revision(
    transaction: &rusqlite::Transaction<'_>,
    plan: &Plan,
    rows: &store_rows::PlanRows,
    revision: Option<i64>,
) -> Result<RowWrites, PlanError> {
    let status = serde_json::to_value(&plan.status)
        .map_err(json_error)?
        .as_str()
        .unwrap_or("active")
        .to_string();
    let previous_header: Option<(String, String, String, String, StoredStrides)> = transaction
        .query_row(
            "SELECT title, status, project_path,
                    coalesce(json_extract(header_json, '$.canonical_project_path'), ''),
                    stride_count, stride_digest, coalesce(json_extract(header_json,'$._stride_digest_invalid'),0)
             FROM plans WHERE id = ?1",
            [&plan.id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    StoredStrides {
                        invalid: row.get(6)?,
                        count: row.get::<_, i64>(4)? as usize,
                        digest: row.get(5)?,
                    },
                ))
            },
        )
        .optional()
        .map_err(db_error)?;
    transaction
        .execute(
            "INSERT INTO plans(id, title, status, team_id, project_path, updated_at, stride_count, stride_digest, header_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, json_set(?9, '$._revision', coalesce(?10,
                 (SELECT json_extract(header_json, '$._revision') FROM plans WHERE id=?1), 0)))
             ON CONFLICT(id) DO UPDATE SET title=excluded.title, status=excluded.status,
             team_id=excluded.team_id,
             project_path=excluded.project_path, updated_at=excluded.updated_at,
             stride_count=excluded.stride_count, stride_digest=excluded.stride_digest,
             header_json=excluded.header_json",
            params![
                plan.id,
                plan.title,
                status,
                plan.team_id,
                plan.project_path,
                plan.updated_at,
                rows.strides.len() as i64,
                rows.stride_digest(),
                rows.header,
                revision
            ],
        )
        .map_err(db_error)?;
    let mut writes =
        store_sync::sync_task_rows(transaction, &plan.id, &rows.tasks).map_err(db_error)?;
    let stored_strides = previous_header
        .as_ref()
        .map(|header| header.4)
        .unwrap_or_default();
    store_sync::sync_plan_level_rows(transaction, &plan.id, rows, stored_strides, &mut writes)
        .map_err(db_error)?;
    if let Some((title, previous_status, project_path, canonical_project_path, _)) = previous_header
    {
        if project_path != plan.project_path
            || canonical_project_path != plan.canonical_project_path
        {
            writes.previous_project_paths = Some((project_path, canonical_project_path));
        }
        writes.header_identity_changed = title != plan.title
            || previous_status != status
            || writes.previous_project_paths.is_some();
    }
    Ok(writes)
}

fn upsert_spec(transaction: &rusqlite::Transaction<'_>, spec: &Spec) -> Result<(), PlanError> {
    let document = serde_json::to_string(spec).map_err(json_error)?;
    transaction
        .execute(
            "INSERT INTO specs(id, plan_id, project_path, updated_at, row_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET plan_id=excluded.plan_id,
             project_path=excluded.project_path, updated_at=excluded.updated_at,
             row_json=excluded.row_json",
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

#[cfg(test)]
fn parse_plan_json(document: &str) -> Result<Plan, String> {
    serde_json::from_str(document).map_err(|error| error.to_string())
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

fn db_error(error: rusqlite::Error) -> PlanError {
    PlanError::Io(error.to_string())
}

fn io_error(error: std::io::Error) -> PlanError {
    PlanError::Io(error.to_string())
}

fn json_error(error: serde_json::Error) -> PlanError {
    PlanError::Io(error.to_string())
}

pub(super) mod admission;
mod attachment_queries;
mod auxiliary_queries;
mod binding_context_queries;
mod binding_queries;
mod change_feed;
mod charter_queries;
mod chat_read_queries;
mod curator_digest_queries;
pub(in crate::service_plan) mod curator_input_queries;
mod curator_notice_queries;
mod curator_queries;
mod curator_snapshot;
mod export_queries;
pub(in crate::service_plan) mod handoff_queries;
mod header_queries;
mod heartbeat_queries;
mod index_queries;
mod locate_queries;
mod notice_queries;
mod qa_queries;
mod reference_resolver_queries;
mod related_queries;
mod search_queries;
mod section_link_queries;
mod task_key_queries;
mod transactions;
mod worker_read_queries;
pub(super) use index_queries::PlanIndexRow;
mod board_state_queries;
mod list_window_queries;
mod match_queries;
pub(super) mod read_projections;
mod reference_queries;
mod state_event_queries;
mod state_event_window;
mod status_queries;
mod task_history_queries;
mod task_list_queries;
mod task_read_queries;
mod ui_projections;
pub use match_queries::{AttemptLanePathRead, SectionRefRow};
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod snapshot_check;
mod startup_queries;
pub(in crate::service_plan) mod task_page_queries;
#[cfg(test)]
mod tests;

mod coverage_queries;
mod document_queries;
pub(in crate::service_plan) mod hot_write;
mod operation_retention;

#[cfg(test)]
pub(super) mod release_upgrade_tests;
mod task_authority_queries;
