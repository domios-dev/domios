pub(crate) mod cron;

use crate::assets::{self, Layout};
use crate::error::{sqlite_error, StorageError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::path::Path;
use std::time::Duration;

mod blueprint;
mod desk;
mod sentinel;

/// `PRAGMA application_id` of every Storage database ("CTST").
pub(crate) const APPLICATION_ID: i32 = 0x4354_5354;

/// Index `n` upgrades schema `n` to `n + 1`; follow-up domain migrations append here.
const MIGRATIONS: &[&str] = &[
    SCHEMA_V1,
    cron::SCHEMA,
    sentinel::SCHEMA,
    desk::SCHEMA,
    // Sentinel graph v4 is the migration immediately after the desk migration.
    sentinel::GRAPH_SCHEMA,
    "ALTER TABLE sentinel_runtime ADD COLUMN run_transitions INTEGER NOT NULL DEFAULT 0 CHECK (run_transitions >= 0);",
    "ALTER TABLE sentinel_buckets ADD COLUMN rearm_on_change INTEGER NOT NULL DEFAULT 0 CHECK (rearm_on_change IN (0, 1));",
    // The source pair lands in one rebuild; later source kinds need no schema change.
    sentinel::SOURCE_SCHEMA,
    sentinel::PIPELINE_SCHEMA,
    "ALTER TABLE sentinel_transitions ADD COLUMN decision_json TEXT CHECK (decision_json IS NULL OR length(decision_json) <= 4096);",
    "ALTER TABLE sentinel_runtime ADD COLUMN causal_run_json TEXT CHECK (causal_run_json IS NULL OR length(causal_run_json) <= 4096);
     ALTER TABLE sentinel_transitions ADD COLUMN causal_run_json TEXT CHECK (causal_run_json IS NULL OR length(causal_run_json) <= 4096);",
    "ALTER TABLE sentinel_definitions ADD COLUMN terminal_spec TEXT CHECK (terminal_spec IS NULL OR length(terminal_spec) <= 8192);
     ALTER TABLE sentinel_definitions ADD COLUMN session INTEGER NOT NULL DEFAULT 0 CHECK (session IN (0, 1));",
    blueprint::SCHEMA,
    "ALTER TABLE blueprint_runtime ADD COLUMN runs_since_transition INTEGER NOT NULL DEFAULT 0 CHECK (runs_since_transition >= 0);
     ALTER TABLE blueprint_runtime ADD COLUMN last_probe_at INTEGER CHECK (last_probe_at IS NULL OR last_probe_at >= 0);
     ALTER TABLE blueprint_runtime ADD COLUMN probe_state TEXT;",
    "ALTER TABLE blueprint_definitions ADD COLUMN model_classification INTEGER NOT NULL DEFAULT 0 CHECK (model_classification IN (0, 1));",
];
pub(crate) const SCHEMA_VERSION: i32 = MIGRATIONS.len() as i32;

/// Every table holding `asset_id` references; an asset is released only when none refers to it.
pub(crate) const ASSET_REFERENCE_TABLES: &[&str] = &["workspace_assets"];

pub(crate) const COMMIT_REVISION: &str = "commit_revision";
pub(crate) const WORKSPACE_ORDER_REVISION: &str = "workspaces.order_revision";

// Foreign keys use immediate NO ACTION: checked at statement end and reported as
// SQLITE_CONSTRAINT_FOREIGNKEY. RESTRICT runs as a RAISE trigger (SQLITE_CONSTRAINT_TRIGGER).
const SCHEMA_V1: &str = r#"
CREATE TABLE storage_meta (
    key TEXT PRIMARY KEY NOT NULL,
    value INTEGER NOT NULL CHECK (value >= 0)
) STRICT;
INSERT INTO storage_meta (key, value)
    VALUES ('commit_revision', 0), ('workspaces.order_revision', 0);
CREATE TABLE workspaces (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) > 0),
    ordinal INTEGER NOT NULL UNIQUE,
    name TEXT NOT NULL,
    cwd TEXT,
    provisioned_cwd TEXT,
    protected INTEGER NOT NULL CHECK (protected IN (0, 1)),
    task_key_prefix TEXT,
    task_key_next INTEGER NOT NULL CHECK (task_key_next >= 0),
    revision INTEGER NOT NULL CHECK (revision > 0)
) STRICT;
CREATE TABLE assets (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 32),
    byte_len INTEGER NOT NULL CHECK (byte_len >= 0),
    created_revision INTEGER NOT NULL
) STRICT;
CREATE TABLE workspace_assets (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    role TEXT NOT NULL,
    asset_id TEXT NOT NULL REFERENCES assets(id),
    PRIMARY KEY (workspace_id, role)
) STRICT;
CREATE INDEX workspace_assets_by_asset ON workspace_assets(asset_id);
CREATE TABLE asset_cleanup (
    asset_id TEXT PRIMARY KEY NOT NULL CHECK (length(asset_id) = 32),
    released_revision INTEGER NOT NULL,
    last_error TEXT
) STRICT;
"#;

/// SQLite `PRAGMA synchronous` level as read back from a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Synchronous {
    Off,
    Normal,
    Full,
    Extra,
    Unknown(i64),
}

/// Settings read back from one live connection after configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSettings {
    pub journal_mode: String,
    pub synchronous: Synchronous,
    pub foreign_keys: bool,
    pub query_only: bool,
    pub busy_timeout: Duration,
}

/// Verified settings of the writer and every owned reader connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSettings {
    pub writer: ConnectionSettings,
    pub readers: Vec<ConnectionSettings>,
}

/// Opens (initializing a genuinely fresh root) and validates the writer connection.
pub(crate) fn open_writer(
    layout: &Layout,
    root: &Path,
    busy_timeout: Duration,
) -> Result<(Connection, ConnectionSettings), StorageError> {
    if !assets::entry_exists(&layout.database)? {
        ensure_fresh(layout, root)?;
        initialize(layout)?;
    }
    let mut conn = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let version = validate_identity(&conn)?;
    check_integrity(&conn)?;
    let settings = configure(&conn, true, busy_timeout)?;
    if version < SCHEMA_VERSION {
        migrate(&mut conn, version)?;
    }
    read_meta(&conn, COMMIT_REVISION)?;
    Ok((conn, settings))
}

pub(crate) fn open_reader(
    layout: &Layout,
    busy_timeout: Duration,
) -> Result<(Connection, ConnectionSettings), StorageError> {
    let conn = open_existing(&layout.database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let settings = configure(&conn, false, busy_timeout)?;
    Ok((conn, settings))
}

/// Opens an existing database without ever creating one. On Windows the bundled
/// `win32-longpath` VFS takes the extended path; the default VFS caps names at MAX_PATH.
pub(crate) fn open_existing(path: &Path, access: OpenFlags) -> Result<Connection, StorageError> {
    let flags = access | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    #[cfg(windows)]
    let opened =
        Connection::open_with_flags_and_vfs(assets::native_path(path), flags, "win32-longpath");
    #[cfg(not(windows))]
    let opened = Connection::open_with_flags(path, flags);
    opened.map_err(|error| sqlite_error("open database", error))
}

/// A missing database is fresh only when no sidecar or asset survives it.
pub(crate) fn ensure_fresh(layout: &Layout, root: &Path) -> Result<(), StorageError> {
    for suffix in ["-wal", "-shm", "-journal"] {
        if assets::entry_exists(&layout.database_sidecar(suffix))? {
            return Err(StorageError::InconsistentRoot {
                root: root.to_path_buf(),
                reason: "database sidecar exists without its database",
            });
        }
    }
    if assets::dir_has_entries(&layout.blobs)? {
        return Err(StorageError::InconsistentRoot {
            root: root.to_path_buf(),
            reason: "assets exist without a database",
        });
    }
    Ok(())
}

/// Builds the full schema in a private file and publishes it, so `state.db` exists only complete.
fn initialize(layout: &Layout) -> Result<(), StorageError> {
    let staging = layout
        .tmp
        .join(format!("init-{:032x}.db", assets::unique_token()));
    assets::create_owner_file(&staging)?;
    let result = (|| -> Result<(), StorageError> {
        let mut conn = open_existing(&staging, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(|e| sqlite_error("configure staging", e))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| sqlite_error("configure staging", e))?;
        migrate(&mut conn, 0)?;
        let mode: String = conn
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
            .map_err(|e| sqlite_error("enable wal", e))?;
        require("journal_mode", "wal", &mode)?;
        conn.close()
            .map_err(|(_, e)| sqlite_error("close staging", e))?;
        assets::sync_file(&staging)?;
        assets::publish_no_replace(&staging, &layout.database, &layout.dir)
    })();
    if result.is_err() {
        for path in [
            staging.clone(),
            suffixed(&staging, "-wal"),
            suffixed(&staging, "-shm"),
            suffixed(&staging, "-journal"),
        ] {
            let _ = std::fs::remove_file(path);
        }
    }
    result
}

fn suffixed(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut text = path.as_os_str().to_os_string();
    text.push(suffix);
    text.into()
}

/// Refuses foreign, empty and newer databases; returns the schema version.
pub(crate) fn validate_identity(conn: &Connection) -> Result<i32, StorageError> {
    let application_id: i32 = pragma_value(conn, "application_id")?;
    let version: i32 = pragma_value(conn, "user_version")?;
    if application_id != APPLICATION_ID || version <= 0 || version > SCHEMA_VERSION {
        return Err(StorageError::Incompatible {
            application_id,
            schema_version: version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(version)
}

pub(crate) fn check_integrity(conn: &Connection) -> Result<(), StorageError> {
    let verdict: String = conn
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|e| sqlite_error("integrity check", e))?;
    if verdict != "ok" {
        return Err(StorageError::Corrupt { detail: verdict });
    }
    Ok(())
}

fn migrate(conn: &mut Connection, from: i32) -> Result<(), StorageError> {
    conn.pragma_update(None, "foreign_keys", "OFF")
        .map_err(|e| sqlite_error("suspend migration foreign keys", e))?;
    let result = migrate_transaction(conn, from);
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| sqlite_error("restore migration foreign keys", e))?;
    result
}

fn migrate_transaction(conn: &mut Connection, from: i32) -> Result<(), StorageError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| sqlite_error("begin migration", e))?;
    for step in MIGRATIONS.iter().skip(from as usize) {
        if *step == sentinel::GRAPH_SCHEMA {
            sentinel::refuse_stored_stages(&tx)?;
        }
        if *step == sentinel::SOURCE_SCHEMA && sentinel::definitions_carry_source_pair(&tx)? {
            continue;
        }
        tx.execute_batch(step)
            .map_err(|e| sqlite_error("apply migration", e))?;
    }
    if from == 0 {
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(|e| sqlite_error("stamp application id", e))?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| sqlite_error("stamp schema version", e))?;
    let violations: i64 = tx
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|e| sqlite_error("verify migration foreign keys", e))?;
    if violations != 0 {
        return Err(StorageError::Corrupt {
            detail: "migration violates foreign keys".into(),
        });
    }
    tx.commit().map_err(|e| sqlite_error("commit migration", e))
}

fn configure(
    conn: &Connection,
    writer: bool,
    busy_timeout: Duration,
) -> Result<ConnectionSettings, StorageError> {
    conn.busy_timeout(busy_timeout)
        .map_err(|e| sqlite_error("configure busy timeout", e))?;
    if writer {
        let _: String = conn
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
            .map_err(|e| sqlite_error("enable wal", e))?;
    }
    let set = |name: &str, value: &str| {
        conn.pragma_update(None, name, value)
            .map_err(|e| sqlite_error("configure connection", e))
    };
    set("synchronous", "FULL")?;
    set("foreign_keys", "ON")?;
    if !writer {
        set("query_only", "ON")?;
    }
    let settings = read_settings(conn)?;
    require("journal_mode", "wal", &settings.journal_mode)?;
    require(
        "synchronous",
        &format!("{:?}", Synchronous::Full),
        &format!("{:?}", settings.synchronous),
    )?;
    require("foreign_keys", "true", &settings.foreign_keys.to_string())?;
    require(
        "query_only",
        &(!writer).to_string(),
        &settings.query_only.to_string(),
    )?;
    let expected_busy = Duration::from_millis(busy_timeout.as_millis() as u64);
    require(
        "busy_timeout",
        &format!("{expected_busy:?}"),
        &format!("{:?}", settings.busy_timeout),
    )?;
    Ok(settings)
}

pub(crate) fn read_settings(conn: &Connection) -> Result<ConnectionSettings, StorageError> {
    let synchronous: i64 = pragma_value(conn, "synchronous")?;
    let busy_ms: i64 = pragma_value(conn, "busy_timeout")?;
    Ok(ConnectionSettings {
        journal_mode: pragma_value::<String>(conn, "journal_mode")?.to_ascii_lowercase(),
        synchronous: match synchronous {
            0 => Synchronous::Off,
            1 => Synchronous::Normal,
            2 => Synchronous::Full,
            3 => Synchronous::Extra,
            other => Synchronous::Unknown(other),
        },
        foreign_keys: pragma_value::<i64>(conn, "foreign_keys")? == 1,
        query_only: pragma_value::<i64>(conn, "query_only")? == 1,
        busy_timeout: Duration::from_millis(busy_ms.max(0) as u64),
    })
}

fn pragma_value<T: rusqlite::types::FromSql>(
    conn: &Connection,
    name: &str,
) -> Result<T, StorageError> {
    conn.pragma_query_value(None, name, |row| row.get(0))
        .map_err(|e| sqlite_error("read pragma", e))
}

fn require(setting: &'static str, expected: &str, actual: &str) -> Result<(), StorageError> {
    if expected == actual {
        return Ok(());
    }
    Err(StorageError::Settings {
        setting,
        expected: expected.to_string(),
        actual: actual.to_string(),
    })
}

pub(crate) fn read_meta(conn: &Connection, key: &str) -> Result<u64, StorageError> {
    let value: Option<i64> = conn
        .prepare_cached("SELECT value FROM storage_meta WHERE key = ?1")
        .and_then(|mut statement| statement.query_row([key], |row| row.get(0)).optional())
        .map_err(|e| sqlite_error("read storage meta", e))?;
    let value = value.ok_or_else(|| StorageError::Corrupt {
        detail: format!("storage meta {key} is missing"),
    })?;
    u64::try_from(value).map_err(|_| StorageError::Corrupt {
        detail: format!("storage meta {key} is negative"),
    })
}

pub(crate) fn write_meta(conn: &Connection, key: &str, value: u64) -> Result<(), StorageError> {
    let value = i64::try_from(value)
        .map_err(|_| StorageError::invalid("revision", "exceeds the signed 64-bit range"))?;
    let changed = conn
        .prepare_cached("UPDATE storage_meta SET value = ?2 WHERE key = ?1")
        .and_then(|mut statement| statement.execute(rusqlite::params![key, value]))
        .map_err(|e| sqlite_error("write storage meta", e))?;
    if changed != 1 {
        return Err(StorageError::Corrupt {
            detail: format!("storage meta {key} is missing"),
        });
    }
    Ok(())
}

/// Truncates the WAL after every other connection closed; returns whether it completed.
pub(crate) fn checkpoint_truncate(conn: &Connection) -> Result<bool, StorageError> {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(|e| sqlite_error("checkpoint", e))?;
    Ok(busy == 0)
}
