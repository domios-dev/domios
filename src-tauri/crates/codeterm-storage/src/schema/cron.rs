//! Cron domain tables. A job identity outlives its definition so retained run history
//! never needs a live job, and a retired identity can never become a live cron again.
//! `cron_last_run_mirrors` holds the current copy of an imported last run that differs
//! from its history row; the original bytes stay in the importer's backup.

/// Registered after the foundation schema in `MIGRATIONS`.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE cron_identities (
    job_id TEXT PRIMARY KEY NOT NULL CHECK (length(job_id) > 0),
    revision INTEGER NOT NULL CHECK (revision > 0),
    retired INTEGER NOT NULL CHECK (retired IN (0, 1))
) STRICT;
CREATE TABLE cron_runs (
    job_id TEXT NOT NULL REFERENCES cron_identities(job_id),
    run_id TEXT NOT NULL CHECK (length(run_id) > 0),
    ordinal INTEGER CHECK (ordinal IS NULL OR ordinal >= 0),
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    finished_at INTEGER CHECK (finished_at IS NULL OR finished_at >= 0),
    status TEXT NOT NULL CHECK (status IN
        ('running', 'awaiting_owner_prompt', 'success', 'failed', 'cancelled')),
    tab_id TEXT,
    session_id TEXT,
    cwd TEXT,
    summary TEXT,
    report_delivery TEXT,
    PRIMARY KEY (job_id, run_id),
    UNIQUE (job_id, ordinal)
) STRICT;
CREATE INDEX cron_runs_by_run ON cron_runs(run_id);
CREATE TABLE cron_jobs (
    id TEXT PRIMARY KEY NOT NULL REFERENCES cron_identities(job_id),
    ordinal INTEGER NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT,
    workspace_id TEXT NOT NULL,
    team_id TEXT,
    schedule TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    agent TEXT NOT NULL,
    model TEXT,
    reasoning_effort TEXT,
    task TEXT NOT NULL,
    cwd TEXT,
    role TEXT,
    skill TEXT,
    creator_tab_id TEXT,
    report_target_tab_id TEXT,
    oneshot INTEGER NOT NULL CHECK (oneshot IN (0, 1)),
    max_history INTEGER NOT NULL CHECK (max_history >= 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    last_run_id TEXT,
    next_run INTEGER CHECK (next_run IS NULL OR next_run >= 0),
    jitter_max_ms INTEGER CHECK (jitter_max_ms IS NULL OR jitter_max_ms >= 0),
    tz TEXT,
    timeout_seconds INTEGER CHECK (timeout_seconds IS NULL OR timeout_seconds >= 0),
    consecutive_failures INTEGER NOT NULL
        CHECK (consecutive_failures BETWEEN 0 AND 4294967295),
    protected INTEGER NOT NULL CHECK (protected IN (0, 1)),
    UNIQUE (id, last_run_id),
    FOREIGN KEY (id, last_run_id) REFERENCES cron_runs(job_id, run_id)
) STRICT;
CREATE INDEX cron_jobs_by_workspace ON cron_jobs(workspace_id);
CREATE INDEX cron_jobs_by_team ON cron_jobs(team_id);
CREATE TABLE cron_last_run_mirrors (
    job_id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL,
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    finished_at INTEGER CHECK (finished_at IS NULL OR finished_at >= 0),
    status TEXT NOT NULL CHECK (status IN
        ('running', 'awaiting_owner_prompt', 'success', 'failed', 'cancelled')),
    tab_id TEXT,
    session_id TEXT,
    cwd TEXT,
    summary TEXT,
    report_delivery TEXT,
    origin TEXT NOT NULL CHECK (origin = 'legacy_import'),
    imported_revision INTEGER NOT NULL CHECK (imported_revision > 0),
    imported_differing TEXT NOT NULL CHECK (length(imported_differing) > 0),
    FOREIGN KEY (job_id, run_id) REFERENCES cron_jobs(id, last_run_id),
    FOREIGN KEY (job_id, run_id) REFERENCES cron_runs(job_id, run_id)
) STRICT;
"#;

#[cfg(feature = "test-support")]
pub(crate) use seed::seed_before_cron;

#[cfg(feature = "test-support")]
mod seed {
    use super::super::{APPLICATION_ID, MIGRATIONS};
    use super::SCHEMA;
    use crate::assets::Layout;
    use crate::error::{sqlite_error, StorageError};
    use rusqlite::Connection;
    use std::path::Path;

    /// Writes a database stamped at the version just before the cron migration, built by
    /// the real migration steps, so opening it must upgrade in place.
    pub(crate) fn seed_before_cron(root: &Path) -> Result<i32, StorageError> {
        let index = MIGRATIONS
            .iter()
            .position(|step| *step == SCHEMA)
            .ok_or_else(|| StorageError::invalid("schema", "cron migration is not registered"))?;
        let layout = Layout::of_root(root);
        layout.create_dirs(false)?;
        let mut conn =
            Connection::open(&layout.database).map_err(|e| sqlite_error("open seed", e))?;
        let version = index as i32;
        let tx = conn
            .transaction()
            .map_err(|e| sqlite_error("begin seed", e))?;
        for step in &MIGRATIONS[..index] {
            tx.execute_batch(step)
                .map_err(|e| sqlite_error("apply seed migration", e))?;
        }
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(|e| sqlite_error("stamp seed", e))?;
        tx.pragma_update(None, "user_version", version)
            .map_err(|e| sqlite_error("stamp seed", e))?;
        tx.commit().map_err(|e| sqlite_error("commit seed", e))?;
        let _: String = conn
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
            .map_err(|e| sqlite_error("enable seed wal", e))?;
        conn.close()
            .map_err(|(_, e)| sqlite_error("close seed", e))?;
        Ok(version)
    }
}
