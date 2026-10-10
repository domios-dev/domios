use crate::repositories::Revision;
use rusqlite::ffi;
use std::fmt;
use std::path::PathBuf;

/// Which SQLite constraint refused a statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    ForeignKey,
    Unique,
    Check,
    NotNull,
    Other,
}

/// Every Storage failure is explicit; none is ever reported as an empty or idle state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StorageError {
    InvalidConfig {
        reason: &'static str,
    },
    RootInvalid {
        root: PathBuf,
        reason: String,
    },
    /// This process already owns the instance root (any spelling of it).
    AlreadyOpen {
        root: PathBuf,
    },
    /// Another owner holds the root's OS lock (another process or an unresolved alias).
    Locked {
        root: PathBuf,
    },
    InconsistentRoot {
        root: PathBuf,
        reason: &'static str,
    },
    Incompatible {
        application_id: i32,
        schema_version: i32,
        supported: i32,
    },
    /// An upgrade would discard stored rows; nothing was migrated.
    MigrationRefused {
        schema_version: i32,
        reason: &'static str,
    },
    Corrupt {
        detail: String,
    },
    Settings {
        setting: &'static str,
        expected: String,
        actual: String,
    },
    Backpressure {
        queued_commands: usize,
        queued_bytes: usize,
        limit_commands: usize,
        limit_bytes: usize,
    },
    TooLarge {
        bytes: usize,
        limit: usize,
    },
    /// Admission is closed because shutdown began or every handle was dropped.
    Closed,
    /// The owner thread stopped before running this operation; it was not applied.
    OwnerStopped,
    /// A Storage call was made from a Storage owner thread, which would self-deadlock.
    Reentrant,
    /// The outcome is unknown to this caller: it may still commit or may have committed.
    Unconfirmed {
        operation: &'static str,
    },
    StaleRevision {
        entity: String,
        expected: Option<Revision>,
        actual: Option<Revision>,
    },
    NotFound {
        entity: String,
    },
    Invalid {
        field: &'static str,
        reason: String,
    },
    Constraint {
        kind: ConstraintKind,
        detail: String,
    },
    DatabaseFull,
    Busy {
        operation: &'static str,
    },
    /// COMMIT failed and the transaction is not visible; nothing was acknowledged.
    CommitFailed {
        detail: String,
    },
    RestoreTargetOccupied {
        root: PathBuf,
    },
    Sqlite {
        operation: &'static str,
        code: i32,
        detail: String,
    },
    Io {
        operation: &'static str,
        kind: std::io::ErrorKind,
        detail: String,
    },
    AssetCorrupt {
        asset: String,
        detail: String,
    },
    Injected {
        point: &'static str,
    },
}

impl StorageError {
    pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        StorageError::Invalid {
            field,
            reason: reason.into(),
        }
    }
}

pub(crate) fn io_error(operation: &'static str, error: std::io::Error) -> StorageError {
    StorageError::Io {
        operation,
        kind: error.kind(),
        detail: error.to_string(),
    }
}

pub(crate) fn sqlite_error(operation: &'static str, error: rusqlite::Error) -> StorageError {
    let rusqlite::Error::SqliteFailure(failure, message) = &error else {
        return StorageError::Sqlite {
            operation,
            code: -1,
            detail: error.to_string(),
        };
    };
    let detail = message.clone().unwrap_or_else(|| failure.to_string());
    match failure.code {
        ffi::ErrorCode::DiskFull => StorageError::DatabaseFull,
        ffi::ErrorCode::DatabaseBusy | ffi::ErrorCode::DatabaseLocked => {
            StorageError::Busy { operation }
        }
        ffi::ErrorCode::DatabaseCorrupt | ffi::ErrorCode::NotADatabase => {
            StorageError::Corrupt { detail }
        }
        ffi::ErrorCode::ConstraintViolation => StorageError::Constraint {
            kind: match failure.extended_code {
                ffi::SQLITE_CONSTRAINT_FOREIGNKEY => ConstraintKind::ForeignKey,
                ffi::SQLITE_CONSTRAINT_UNIQUE | ffi::SQLITE_CONSTRAINT_PRIMARYKEY => {
                    ConstraintKind::Unique
                }
                ffi::SQLITE_CONSTRAINT_CHECK => ConstraintKind::Check,
                ffi::SQLITE_CONSTRAINT_NOTNULL => ConstraintKind::NotNull,
                _ => ConstraintKind::Other,
            },
            detail,
        },
        _ => StorageError::Sqlite {
            operation,
            code: failure.extended_code,
            detail,
        },
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use StorageError as E;
        match self {
            E::InvalidConfig { reason } => write!(f, "invalid storage config: {reason}"),
            E::RootInvalid { root, reason } => write!(f, "root {}: {reason}", root.display()),
            E::AlreadyOpen { root } => write!(f, "root {} is open in-process", root.display()),
            E::Locked { root } => write!(f, "root {} is owned elsewhere", root.display()),
            E::InconsistentRoot { root, reason } => write!(f, "root {}: {reason}", root.display()),
            E::Incompatible {
                application_id,
                schema_version,
                supported,
            } => write!(
                f,
                "incompatible database {application_id:#x} schema {schema_version}/{supported}"
            ),
            E::MigrationRefused {
                schema_version,
                reason,
            } => write!(f, "upgrade from schema {schema_version} refused: {reason}"),
            E::Corrupt { detail } => write!(f, "database is corrupt: {detail}"),
            E::Settings {
                setting,
                expected,
                actual,
            } => write!(f, "{setting} is {actual}, required {expected}"),
            E::Backpressure {
                queued_commands,
                queued_bytes,
                limit_commands,
                limit_bytes,
            } => write!(
                f,
                "mailbox full: {queued_commands}/{limit_commands}, {queued_bytes}/{limit_bytes} B"
            ),
            E::TooLarge { bytes, limit } => write!(f, "{bytes} bytes exceed {limit}"),
            E::Closed => write!(f, "storage admission is closed"),
            E::OwnerStopped => write!(f, "storage owner stopped before running it"),
            E::Reentrant => write!(f, "storage call from a storage owner thread"),
            E::Unconfirmed { operation } => write!(f, "{operation} is unconfirmed"),
            E::StaleRevision {
                entity,
                expected,
                actual,
            } => write!(f, "{entity} is at {actual:?}, expected {expected:?}"),
            E::NotFound { entity } => write!(f, "{entity} not found"),
            E::Invalid { field, reason } => write!(f, "invalid {field}: {reason}"),
            E::Constraint { kind, detail } => write!(f, "{kind:?} constraint: {detail}"),
            E::DatabaseFull => write!(f, "database is full"),
            E::Busy { operation } => write!(f, "{operation} exceeded the busy deadline"),
            E::CommitFailed { detail } => write!(f, "commit failed: {detail}"),
            E::RestoreTargetOccupied { root } => write!(f, "{} holds storage", root.display()),
            E::Sqlite {
                operation,
                code,
                detail,
            } => write!(f, "{operation} failed ({code}): {detail}"),
            E::Io {
                operation,
                kind,
                detail,
            } => write!(f, "{operation} failed ({kind:?}): {detail}"),
            E::AssetCorrupt { asset, detail } => write!(f, "asset {asset} is corrupt: {detail}"),
            E::Injected { point } => write!(f, "injected failure at {point}"),
        }
    }
}

impl std::error::Error for StorageError {}
