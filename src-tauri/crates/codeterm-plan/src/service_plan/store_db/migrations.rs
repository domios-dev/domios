use super::{admission, db_error, PlanError, SCHEMA_VERSION as TARGET_VERSION};
use crate::service_plan::store_convert_118::{self, ConversionReport};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::path::Path;

struct Step {
    version: i64,
    sql: &'static str,
    schema: &'static str,
    preconditions: &'static [(&'static str, &'static str)],
}

const STEPS: &[Step] = &[
    Step {
        version: 1,
        sql: include_str!("migrations/001-row-schema.sql"),
        schema: include_str!("migrations/schemas/001.sql"),
        preconditions: &[],
    },
    Step {
        version: 2,
        sql: include_str!("migrations/002-v1-bridge.sql"),
        schema: include_str!("migrations/schemas/002.sql"),
        preconditions: &[],
    },
    Step {
        version: 3,
        sql: include_str!("migrations/003-planner-tab-index.sql"),
        schema: include_str!("migrations/schemas/003.sql"),
        preconditions: &[],
    },
    Step {
        version: 4,
        sql: include_str!("migrations/004-lane-tab-index.sql"),
        schema: include_str!("migrations/schemas/004.sql"),
        preconditions: &[],
    },
    Step {
        version: 5,
        sql: include_str!("migrations/005-board-event-indexes.sql"),
        schema: include_str!("migrations/schemas/005.sql"),
        preconditions: &[],
    },
];

pub(super) const SCHEMA_VERSION: i64 = STEPS[STEPS.len() - 1].version;
const APPLICATION_ID: i64 = 0x44504c4e;
const V1_WITHOUT_FTS: &str = include_str!("migrations/schemas/published-v1.18.7.sql");

mod backup;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::service_plan) enum Source {
    Empty,
    Legacy,
    Version(i64),
}

fn version(connection: &Connection) -> Result<i64, PlanError> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(db_error)
}

fn matches_schema(connection: &Connection, sql: &str) -> Result<bool, PlanError> {
    let expected = Connection::open_in_memory().map_err(db_error)?;
    expected.execute_batch(sql).map_err(db_error)?;
    let identity = |connection: &Connection| {
        connection.query_row("PRAGMA application_id", [], |row| row.get::<_, i64>(0))
    };
    Ok(admission::schema(connection).map_err(db_error)?
        == admission::schema(&expected).map_err(db_error)?
        && identity(connection).map_err(db_error)? == identity(&expected).map_err(db_error)?)
}

pub(in crate::service_plan) fn validate_source(
    connection: &Connection,
) -> Result<Source, PlanError> {
    let version = version(connection)?;
    let application_id: i64 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(db_error)?;
    let expected_identity = if version < 2 { 0 } else { APPLICATION_ID };
    if application_id != expected_identity {
        return Err(PlanError::Io(
            "foreign plan database application identity".into(),
        ));
    }
    let empty: bool = connection
        .query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM sqlite_schema)",
            [],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if version == 0 && empty {
        return Ok(Source::Empty);
    }
    if version == 0 {
        store_convert_118::validate_legacy(connection)?;
        return Ok(Source::Legacy);
    }
    let step = STEPS
        .iter()
        .find(|step| step.version == version)
        .ok_or_else(|| PlanError::Io(format!("unsupported plan database version {version}")))?;
    if matches_schema(connection, step.schema)?
        || (version == 1 && matches_schema(connection, V1_WITHOUT_FTS)?)
    {
        return Ok(Source::Version(version));
    }
    Err(PlanError::Io(
        "unrecognized, partial or mixed plan database schema".into(),
    ))
}

pub(in crate::service_plan) fn create_v1(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(STEPS[0].sql)
}

pub(in crate::service_plan) fn initialize(
    connection: &mut Connection,
    path: &Path,
) -> Result<Option<ConversionReport>, PlanError> {
    validate_source(connection)?;
    *connection = Connection::open(path).map_err(db_error)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(db_error)?;
    migrate_with(connection, Some(path), |_| Ok(()))
}

fn migrate_with(
    connection: &mut Connection,
    backup_path: Option<&Path>,
    mut after_step: impl FnMut(&Transaction<'_>) -> Result<(), PlanError>,
) -> Result<Option<ConversionReport>, PlanError> {
    let source = validate_source(connection)?;
    if let Some(path) = backup_path {
        let backup = match source {
            Source::Legacy => Some(store_convert_118::back_up(connection, path)?),
            Source::Version(version) if version < TARGET_VERSION => {
                Some(backup::create(connection, path)?)
            }
            _ => None,
        };
        if let Some(backup) = backup {
            log::info!("[plan.store] pre-migration backup: {}", backup.display());
        }
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if validate_source(&transaction)? != source {
        return Err(PlanError::Io(
            "plan migration refused: source changed after backup".into(),
        ));
    }
    let (source_version, report) = match source {
        Source::Empty => (0, None),
        Source::Legacy => (1, Some(store_convert_118::convert(&transaction)?)),
        Source::Version(version) => (version, None),
    };
    for step in STEPS.iter().filter(|step| step.version > source_version) {
        apply_step(&transaction, step)?;
        after_step(&transaction)?;
        if !matches_schema(&transaction, step.schema)? {
            return Err(PlanError::Io(format!(
                "plan migration {} produced an unexpected schema",
                step.version
            )));
        }
    }
    if !matches_schema(&transaction, STEPS[STEPS.len() - 1].schema)? {
        return Err(PlanError::Io(
            "plan migration target schema mismatch".into(),
        ));
    }
    let foreign_key_violation: bool = transaction
        .prepare("PRAGMA foreign_key_check")
        .map_err(db_error)?
        .exists([])
        .map_err(db_error)?;
    if foreign_key_violation {
        return Err(PlanError::Io("plan migration foreign key violation".into()));
    }
    if source_version != TARGET_VERSION {
        transaction
            .pragma_update(None, "user_version", TARGET_VERSION)
            .map_err(db_error)?;
    }
    transaction.commit().map_err(db_error)?;
    Ok(report)
}

fn apply_step(transaction: &Transaction<'_>, step: &Step) -> Result<(), PlanError> {
    for &(sql, reason) in step.preconditions {
        let violated: bool = transaction
            .query_row(sql, [], |row| row.get(0))
            .map_err(db_error)?;
        if violated {
            return Err(PlanError::Io(format!(
                "plan migration {} refused: {reason}",
                step.version
            )));
        }
    }
    transaction.execute_batch(step.sql).map_err(db_error)
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
