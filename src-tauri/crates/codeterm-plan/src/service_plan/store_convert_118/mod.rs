//! One-time conversion of a 1.17 plans.db into the 1.18 row schema. This module is
//! the only reader of the legacy plan-document shape; 1.18.1 deletes it whole.

use super::store_db::{self, SCHEMA_VERSION};
use super::store_rows::{self, join_plan, StoredTask};
use super::store_sync::{grouped_documents, parse};
use super::{Plan, PlanError};
use rusqlite::{params, Connection, DatabaseName, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

const BACKUP_SUFFIX: &str = ".pre-1.18";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ConversionReport {
    pub(super) converted: usize,
    pub(super) quarantined: usize,
}

/// Backs a legacy database up beside itself, then converts it in one transaction.
/// A database without the legacy plans table is fresh and left to the schema setup.
pub(super) fn convert_legacy_database(
    connection: &mut Connection,
    path: &Path,
) -> Result<Option<ConversionReport>, PlanError> {
    if !has_legacy_plans(connection).map_err(db_error)? {
        return Ok(None);
    }
    let started = Instant::now();
    back_up(connection, path)?;
    let report = convert(connection)?;
    log::info!(
        "[plan.store] 1.18 conversion converted={} quarantined={} elapsed_ms={}",
        report.converted,
        report.quarantined,
        started.elapsed().as_millis()
    );
    Ok(Some(report))
}

fn has_legacy_plans(connection: &Connection) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('plans') WHERE name = 'document_json')",
        [],
        |row| row.get(0),
    )
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(BACKUP_SUFFIX);
    PathBuf::from(name)
}

/// Writes a consistent online copy to `plans.db.pre-1.18`; any failure refuses the conversion.
fn back_up(connection: &Connection, path: &Path) -> Result<PathBuf, PlanError> {
    let backup = backup_path(path);
    let mut staging = backup.clone().into_os_string();
    staging.push(".partial");
    let staging = PathBuf::from(staging);
    let copied = (|| -> Result<(), String> {
        if staging.exists() {
            std::fs::remove_file(&staging).map_err(|error| error.to_string())?;
        }
        connection
            .backup(DatabaseName::Main, &staging, None)
            .map_err(|error| error.to_string())?;
        std::fs::rename(&staging, &backup).map_err(|error| error.to_string())
    })();
    if let Err(error) = copied {
        let _ = std::fs::remove_file(&staging);
        return Err(PlanError::Io(format!(
            "1.18 plan storage conversion refused: backup to {} failed: {error}",
            backup.display()
        )));
    }
    Ok(backup)
}

struct LegacySpec {
    id: String,
    plan_id: Option<String>,
    project_path: String,
    updated_at: i64,
    document: String,
}

fn convert(connection: &mut Connection) -> Result<ConversionReport, PlanError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let plans = read_legacy_plans(&transaction).map_err(db_error)?;
    let specs = read_legacy_specs(&transaction).map_err(db_error)?;
    transaction
        .execute_batch(
            "DROP TABLE plan_task_milestones;
             DROP TABLE plan_task_attempts;
             DROP TABLE plan_tasks;
             DROP TABLE specs;
             DROP TABLE plans;",
        )
        .map_err(db_error)?;
    store_db::create_schema(&transaction).map_err(db_error)?;

    let mut report = ConversionReport::default();
    let mut converted = HashSet::new();
    for (plan_id, plan) in plans {
        let split = plan.and_then(|plan| {
            let value = serde_json::to_value(&plan).map_err(|error| error.to_string())?;
            store_rows::split_plan(value).map(|rows| (plan, rows))
        });
        match split {
            Ok((plan, rows)) => {
                store_db::write_plan_rows(&transaction, &plan, &rows)?;
                converted.insert(plan_id);
                report.converted += 1;
            }
            Err(error) => {
                log::error!(
                    "[plan.store] quarantined plan record {plan_id} during the 1.18 conversion: {error}; kept only in the {BACKUP_SUFFIX} backup"
                );
                report.quarantined += 1;
            }
        }
    }
    for spec in specs {
        if let Some(plan_id) = spec
            .plan_id
            .as_ref()
            .filter(|plan_id| !converted.contains(*plan_id))
        {
            log::error!(
                "[plan.store] dropped spec {} of unconverted plan {plan_id} during the 1.18 conversion; kept only in the {BACKUP_SUFFIX} backup",
                spec.id
            );
            continue;
        }
        transaction
            .execute(
                "INSERT INTO specs(id, plan_id, project_path, updated_at, row_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    spec.id,
                    spec.plan_id,
                    spec.project_path,
                    spec.updated_at,
                    spec.document
                ],
            )
            .map_err(db_error)?;
    }
    transaction
        .execute(
            "DELETE FROM plan_fts WHERE plan_id NOT IN (SELECT id FROM plans)",
            [],
        )
        .map_err(db_error)?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(db_error)?;
    transaction.commit().map_err(db_error)?;
    Ok(report)
}

fn read_legacy_plans(
    connection: &Connection,
) -> rusqlite::Result<Vec<(String, Result<Plan, String>)>> {
    let mut statement = connection.prepare("SELECT id FROM plans ORDER BY id")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids
        .into_iter()
        .map(|id| {
            let plan = read_legacy_plan(connection, &id);
            (id, plan)
        })
        .collect())
}

/// The 1.17 load of one plan: the conversion input and the round-trip oracle.
fn read_legacy_plan(connection: &Connection, plan_id: &str) -> Result<Plan, String> {
    let document = plan_document(connection, plan_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("plan row {plan_id} is missing"))?;
    let value = load_plan_value(connection, plan_id, &document)?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn read_legacy_specs(connection: &Connection) -> rusqlite::Result<Vec<LegacySpec>> {
    let mut statement = connection
        .prepare("SELECT id, plan_id, project_path, updated_at, document_json FROM specs")?;
    let specs = statement.query_map([], |row| {
        Ok(LegacySpec {
            id: row.get(0)?,
            plan_id: row.get(1)?,
            project_path: row.get(2)?,
            updated_at: row.get(3)?,
            document: row.get(4)?,
        })
    })?;
    specs.collect()
}

fn db_error(error: rusqlite::Error) -> PlanError {
    PlanError::Io(error.to_string())
}

// The 1.17 legacy reader, moved from store_sync unchanged except visibility.

/// Joins a plan row with its task rows into the full plan document.
fn load_plan_value(
    connection: &Connection,
    plan_id: &str,
    document: &str,
) -> Result<Value, String> {
    let plan: Value = serde_json::from_str(document).map_err(|error| error.to_string())?;
    let mut milestones = grouped_documents(
        connection,
        "SELECT task_id, document_json FROM plan_task_milestones WHERE plan_id = ?1 ORDER BY task_id, seq",
        plan_id,
    )?;
    let mut attempts = grouped_documents(
        connection,
        "SELECT task_id, document_json FROM plan_task_attempts WHERE plan_id = ?1 ORDER BY task_id, position",
        plan_id,
    )?;
    let mut statement = connection
        .prepare_cached(
            "SELECT task_id, document_json FROM plan_tasks WHERE plan_id = ?1 ORDER BY position",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([plan_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut tasks = Vec::new();
    for row in rows {
        let (task_id, document) = row.map_err(|error| error.to_string())?;
        tasks.push(StoredTask {
            document: parse(&document)?,
            milestones: milestones.remove(&task_id).unwrap_or_default(),
            attempts: attempts.remove(&task_id).unwrap_or_default(),
        });
    }
    join_plan(plan, tasks)
}

fn plan_document(connection: &Connection, plan_id: &str) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT document_json FROM plans WHERE id = ?1",
            [plan_id],
            |row| row.get(0),
        )
        .optional()
}

#[cfg(test)]
mod tests;
