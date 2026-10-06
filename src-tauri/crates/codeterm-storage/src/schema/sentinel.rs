//! Sentinel scenarios: stable identity, immutable definition versions behind a mutable head,
//! mutable stage runtime, accepted transitions and independent handler claims/results.

use crate::error::{sqlite_error, StorageError};
use rusqlite::Connection;

// Every relationship is an immediate NO ACTION foreign key; deletions are explicit and ordered.
// Target identities are historical references, never keys into live tabs, crons or Teams.
pub(super) const SCHEMA: &str = r#"
CREATE TABLE sentinels (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) > 0),
    ordinal INTEGER NOT NULL UNIQUE CHECK (ordinal >= 0),
    creator_tab_id TEXT NOT NULL CHECK (length(creator_tab_id) > 0),
    workspace_id TEXT NOT NULL CHECK (length(workspace_id) > 0),
    team_id TEXT CHECK (team_id IS NULL OR length(team_id) > 0),
    cwd TEXT NOT NULL CHECK (length(cwd) > 0),
    agent_authored INTEGER NOT NULL CHECK (agent_authored IN (0, 1)),
    scenario_id TEXT REFERENCES sentinels(id),
    revision INTEGER NOT NULL CHECK (revision > 0)
) STRICT;
CREATE INDEX sentinels_by_scope ON sentinels(workspace_id, team_id);
CREATE INDEX sentinels_by_team ON sentinels(team_id);
CREATE INDEX sentinels_by_scenario ON sentinels(scenario_id);
CREATE TABLE sentinel_definitions (
    sentinel_id TEXT NOT NULL REFERENCES sentinels(id),
    version INTEGER NOT NULL CHECK (version > 0),
    provenance TEXT NOT NULL CHECK (provenance IN ('authored', 'imported')),
    name TEXT,
    probe TEXT NOT NULL,
    question TEXT NOT NULL,
    interval_ms INTEGER NOT NULL CHECK (interval_ms > 0),
    until_bucket TEXT,
    max_transitions INTEGER NOT NULL CHECK (max_transitions BETWEEN 1 AND 65535),
    max_actions INTEGER NOT NULL CHECK (max_actions BETWEEN 1 AND 65535),
    created_revision INTEGER NOT NULL CHECK (created_revision > 0),
    PRIMARY KEY (sentinel_id, version)
) STRICT;
CREATE TABLE sentinel_heads (
    sentinel_id TEXT PRIMARY KEY NOT NULL REFERENCES sentinels(id),
    version INTEGER NOT NULL,
    FOREIGN KEY (sentinel_id, version) REFERENCES sentinel_definitions(sentinel_id, version)
) STRICT;
CREATE TABLE sentinel_buckets (
    sentinel_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    bucket TEXT NOT NULL CHECK (length(bucket) > 0),
    match_pattern TEXT,
    PRIMARY KEY (sentinel_id, version, ordinal),
    UNIQUE (sentinel_id, version, bucket),
    FOREIGN KEY (sentinel_id, version) REFERENCES sentinel_definitions(sentinel_id, version)
) STRICT;
CREATE TABLE sentinel_handlers (
    sentinel_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    bucket TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    handler_id TEXT NOT NULL CHECK (length(handler_id) > 0),
    kind TEXT NOT NULL CHECK (kind IN ('notify', 'send', 'cron', 'sentinel', 'stop', 'command')),
    target_ref TEXT,
    command TEXT,
    label TEXT,
    timeout_ms INTEGER,
    PRIMARY KEY (sentinel_id, version, bucket, ordinal),
    UNIQUE (sentinel_id, version, handler_id),
    FOREIGN KEY (sentinel_id, version, bucket)
        REFERENCES sentinel_buckets(sentinel_id, version, bucket),
    CHECK ((kind IN ('send', 'cron', 'sentinel')) = (target_ref IS NOT NULL)),
    CHECK ((kind = 'command') = (command IS NOT NULL)),
    CHECK ((command IS NULL) = (label IS NULL) AND (command IS NULL) = (timeout_ms IS NULL))
) STRICT;
CREATE TABLE sentinel_handler_targets (
    sentinel_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    handler_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (length(kind) > 0),
    target_id TEXT NOT NULL CHECK (length(target_id) > 0),
    workspace_id TEXT NOT NULL,
    team_id TEXT,
    owner_tab_id TEXT,
    PRIMARY KEY (sentinel_id, version, handler_id),
    FOREIGN KEY (sentinel_id, version, handler_id)
        REFERENCES sentinel_handlers(sentinel_id, version, handler_id)
) STRICT;
CREATE TABLE sentinel_runtime (
    sentinel_id TEXT PRIMARY KEY NOT NULL REFERENCES sentinels(id),
    generation INTEGER NOT NULL CHECK (generation >= 0),
    next_run INTEGER NOT NULL CHECK (next_run >= 0),
    expires_at INTEGER CHECK (expires_at IS NULL OR expires_at >= 0),
    paused INTEGER NOT NULL CHECK (paused IN (0, 1)),
    stopped INTEGER NOT NULL CHECK (stopped IN (0, 1)),
    last_hash TEXT,
    last_bucket TEXT,
    last_reason TEXT
) STRICT;
CREATE TABLE sentinel_transitions (
    sentinel_id TEXT NOT NULL REFERENCES sentinels(id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    transition_id TEXT NOT NULL CHECK (length(transition_id) > 0),
    bucket TEXT NOT NULL,
    accepted_at INTEGER NOT NULL CHECK (accepted_at >= 0),
    terminal INTEGER NOT NULL CHECK (terminal IN (0, 1)),
    definition_version INTEGER,
    PRIMARY KEY (sentinel_id, ordinal),
    UNIQUE (sentinel_id, transition_id),
    FOREIGN KEY (sentinel_id, definition_version)
        REFERENCES sentinel_definitions(sentinel_id, version)
) STRICT;
CREATE TABLE sentinel_handler_results (
    sentinel_id TEXT NOT NULL,
    transition_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    handler_id TEXT NOT NULL CHECK (length(handler_id) > 0),
    status TEXT NOT NULL
        CHECK (status IN ('unconfirmed', 'completed', 'failed', 'noop', 'skipped')),
    claim_id TEXT,
    execution_id TEXT,
    accepted_at INTEGER,
    reason TEXT,
    PRIMARY KEY (sentinel_id, transition_id, ordinal),
    UNIQUE (sentinel_id, transition_id, handler_id),
    FOREIGN KEY (sentinel_id, transition_id)
        REFERENCES sentinel_transitions(sentinel_id, transition_id),
    CHECK ((claim_id IS NULL) = (accepted_at IS NULL)),
    CHECK ((status = 'skipped') = (claim_id IS NULL)),
    CHECK (status <> 'skipped' OR execution_id IS NULL)
) STRICT;
CREATE TABLE sentinel_command_results (
    sentinel_id TEXT NOT NULL,
    transition_id TEXT NOT NULL,
    handler_id TEXT NOT NULL,
    status TEXT NOT NULL
        CHECK (status IN ('success', 'nonzero', 'error', 'cancelled', 'deadline', 'unconfirmed')),
    exit_code INTEGER,
    resources_confirmed INTEGER NOT NULL CHECK (resources_confirmed IN (0, 1)),
    stdout_total INTEGER,
    stdout_retained INTEGER,
    stdout_truncated INTEGER,
    stderr_total INTEGER,
    stderr_retained INTEGER,
    stderr_truncated INTEGER,
    started_at INTEGER NOT NULL,
    finished_at INTEGER NOT NULL,
    primary_failure TEXT,
    PRIMARY KEY (sentinel_id, transition_id, handler_id),
    FOREIGN KEY (sentinel_id, transition_id, handler_id)
        REFERENCES sentinel_handler_results(sentinel_id, transition_id, handler_id),
    CHECK ((stdout_total IS NULL) = (stdout_retained IS NULL)
        AND (stdout_total IS NULL) = (stdout_truncated IS NULL)),
    CHECK ((stderr_total IS NULL) = (stderr_retained IS NULL)
        AND (stderr_total IS NULL) = (stderr_truncated IS NULL))
) STRICT;
CREATE TABLE sentinel_command_cleanup (
    sentinel_id TEXT NOT NULL,
    transition_id TEXT NOT NULL,
    handler_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    error TEXT NOT NULL,
    PRIMARY KEY (sentinel_id, transition_id, handler_id, ordinal),
    FOREIGN KEY (sentinel_id, transition_id, handler_id)
        REFERENCES sentinel_command_results(sentinel_id, transition_id, handler_id)
) STRICT;
"#;

/// Rebuilds the Sentinel tables for graph stages; `refuse_stored_stages` runs first, so the
/// rebuild never discards a stored stage.
pub(super) const GRAPH_SCHEMA: &str = r#"
DROP TABLE sentinel_command_cleanup;
DROP TABLE sentinel_command_results;
DROP TABLE sentinel_handler_results;
DROP TABLE sentinel_transitions;
DROP TABLE sentinel_handler_targets;
DROP TABLE sentinel_handlers;
DROP TABLE sentinel_buckets;
DROP TABLE sentinel_heads;
DROP TABLE sentinel_runtime;
DROP TABLE sentinel_definitions;
DROP TABLE sentinels;
CREATE TABLE sentinels (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) > 0),
    ordinal INTEGER NOT NULL UNIQUE CHECK (ordinal >= 0),
    creator_tab_id TEXT NOT NULL CHECK (length(creator_tab_id) > 0),
    workspace_id TEXT NOT NULL CHECK (length(workspace_id) > 0),
    team_id TEXT CHECK (team_id IS NULL OR length(team_id) > 0),
    cwd TEXT NOT NULL CHECK (length(cwd) > 0),
    agent_authored INTEGER NOT NULL CHECK (agent_authored IN (0, 1)),
    scenario_id TEXT REFERENCES sentinels(id),
    revision INTEGER NOT NULL CHECK (revision > 0)
) STRICT;
CREATE INDEX sentinels_by_scope ON sentinels(workspace_id, team_id);
CREATE INDEX sentinels_by_team ON sentinels(team_id);
CREATE INDEX sentinels_by_scenario ON sentinels(scenario_id);
CREATE TABLE sentinel_definitions (
    sentinel_id TEXT NOT NULL REFERENCES sentinels(id),
    version INTEGER NOT NULL CHECK (version > 0),
    provenance TEXT NOT NULL CHECK (provenance IN ('authored', 'imported')),
    name TEXT,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('probe', 'observe', 'input', 'event')),
    source_spec TEXT,
    probe TEXT NOT NULL,
    observe_kind TEXT CHECK (observe_kind IN ('tab', 'cron_run')),
    observe_ref TEXT CHECK (observe_ref IS NULL OR length(observe_ref) > 0),
    question TEXT NOT NULL,
    interval_ms INTEGER NOT NULL CHECK (interval_ms > 0),
    until_bucket TEXT,
    escalation_timeout_ms INTEGER
        CHECK (escalation_timeout_ms IS NULL OR escalation_timeout_ms > 0),
    max_transitions INTEGER NOT NULL CHECK (max_transitions BETWEEN 1 AND 65535),
    max_actions INTEGER NOT NULL CHECK (max_actions BETWEEN 1 AND 65535),
    replaced_version INTEGER,
    author_tab_id TEXT CHECK (author_tab_id IS NULL OR length(author_tab_id) > 0),
    cause_transition_id TEXT
        CHECK (cause_transition_id IS NULL OR length(cause_transition_id) > 0),
    authored_at INTEGER CHECK (authored_at IS NULL OR authored_at >= 0),
    created_revision INTEGER NOT NULL CHECK (created_revision > 0),
    PRIMARY KEY (sentinel_id, version),
    CHECK ((source_kind = 'probe') = (length(probe) > 0)),
    CHECK ((source_kind = 'observe') = (observe_kind IS NOT NULL)),
    CHECK ((source_kind IN ('input', 'event')) = (source_spec IS NOT NULL)),
    CHECK ((observe_kind IS NULL) = (observe_ref IS NULL)),
    CHECK (replaced_version IS NULL OR (replaced_version > 0 AND replaced_version < version)),
    CHECK ((replaced_version IS NULL) = (author_tab_id IS NULL)
        AND (replaced_version IS NULL) = (authored_at IS NULL)),
    CHECK (cause_transition_id IS NULL OR replaced_version IS NOT NULL)
) STRICT;
CREATE TABLE sentinel_heads (
    sentinel_id TEXT PRIMARY KEY NOT NULL REFERENCES sentinels(id),
    version INTEGER NOT NULL,
    FOREIGN KEY (sentinel_id, version) REFERENCES sentinel_definitions(sentinel_id, version)
) STRICT;
CREATE TABLE sentinel_buckets (
    sentinel_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    bucket TEXT NOT NULL CHECK (length(bucket) > 0),
    match_pattern TEXT,
    PRIMARY KEY (sentinel_id, version, ordinal),
    UNIQUE (sentinel_id, version, bucket),
    FOREIGN KEY (sentinel_id, version) REFERENCES sentinel_definitions(sentinel_id, version),
    CHECK (bucket <> 'other' OR match_pattern IS NULL)
) STRICT;
CREATE TABLE sentinel_handlers (
    sentinel_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    bucket TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    handler_id TEXT NOT NULL CHECK (length(handler_id) > 0),
    kind TEXT NOT NULL CHECK (kind IN ('notify', 'send', 'cron', 'sentinel', 'stop', 'command',
        'respond_prompt', 'escalate')),
    target_ref TEXT,
    command TEXT,
    label TEXT,
    timeout_ms INTEGER,
    safe_kind TEXT CHECK (safe_kind IS NULL OR length(safe_kind) > 0),
    escalate_to TEXT CHECK (escalate_to IN ('creator', 'agent')),
    PRIMARY KEY (sentinel_id, version, bucket, ordinal),
    UNIQUE (sentinel_id, version, handler_id),
    FOREIGN KEY (sentinel_id, version, bucket)
        REFERENCES sentinel_buckets(sentinel_id, version, bucket),
    CHECK ((kind IN ('send', 'cron', 'sentinel') OR escalate_to IS 'agent')
        = (target_ref IS NOT NULL)),
    CHECK ((kind = 'command') = (command IS NOT NULL)),
    CHECK ((command IS NULL) = (label IS NULL) AND (command IS NULL) = (timeout_ms IS NULL)),
    CHECK ((kind = 'respond_prompt') = (safe_kind IS NOT NULL)),
    CHECK ((kind = 'escalate') = (escalate_to IS NOT NULL))
) STRICT;
CREATE TABLE sentinel_handler_targets (
    sentinel_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    handler_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (length(kind) > 0),
    target_id TEXT NOT NULL CHECK (length(target_id) > 0),
    workspace_id TEXT NOT NULL,
    team_id TEXT,
    owner_tab_id TEXT,
    PRIMARY KEY (sentinel_id, version, handler_id),
    FOREIGN KEY (sentinel_id, version, handler_id)
        REFERENCES sentinel_handlers(sentinel_id, version, handler_id)
) STRICT;
CREATE TABLE sentinel_runtime (
    sentinel_id TEXT PRIMARY KEY NOT NULL REFERENCES sentinels(id),
    generation INTEGER NOT NULL CHECK (generation >= 0),
    next_run INTEGER NOT NULL CHECK (next_run >= 0),
    expires_at INTEGER CHECK (expires_at IS NULL OR expires_at >= 0),
    paused INTEGER NOT NULL CHECK (paused IN (0, 1)),
    stopped INTEGER NOT NULL CHECK (stopped IN (0, 1)),
    last_hash TEXT,
    last_bucket TEXT,
    last_reason TEXT,
    held TEXT CHECK (held IS NULL OR length(held) > 0),
    run_id TEXT CHECK (run_id IS NULL OR length(run_id) > 0),
    run_tab_id TEXT CHECK (run_tab_id IS NULL OR length(run_tab_id) > 0),
    run_bound_at INTEGER CHECK (run_bound_at IS NULL OR run_bound_at >= 0),
    CHECK ((run_id IS NULL) = (run_tab_id IS NULL)
        AND (run_id IS NULL) = (run_bound_at IS NULL))
) STRICT;
CREATE TABLE sentinel_transitions (
    sentinel_id TEXT NOT NULL REFERENCES sentinels(id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    transition_id TEXT NOT NULL CHECK (length(transition_id) > 0),
    bucket TEXT NOT NULL,
    accepted_at INTEGER NOT NULL CHECK (accepted_at >= 0),
    terminal INTEGER NOT NULL CHECK (terminal IN (0, 1)),
    definition_version INTEGER,
    PRIMARY KEY (sentinel_id, ordinal),
    UNIQUE (sentinel_id, transition_id),
    FOREIGN KEY (sentinel_id, definition_version)
        REFERENCES sentinel_definitions(sentinel_id, version)
) STRICT;
CREATE TABLE sentinel_handler_results (
    sentinel_id TEXT NOT NULL,
    transition_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    handler_id TEXT NOT NULL CHECK (length(handler_id) > 0),
    status TEXT NOT NULL
        CHECK (status IN ('unconfirmed', 'completed', 'failed', 'noop', 'skipped')),
    claim_id TEXT,
    execution_id TEXT,
    accepted_at INTEGER,
    reason TEXT,
    PRIMARY KEY (sentinel_id, transition_id, ordinal),
    UNIQUE (sentinel_id, transition_id, handler_id),
    FOREIGN KEY (sentinel_id, transition_id)
        REFERENCES sentinel_transitions(sentinel_id, transition_id),
    CHECK ((claim_id IS NULL) = (accepted_at IS NULL)),
    CHECK ((status = 'skipped') = (claim_id IS NULL)),
    CHECK (status <> 'skipped' OR execution_id IS NULL)
) STRICT;
CREATE TABLE sentinel_command_results (
    sentinel_id TEXT NOT NULL,
    transition_id TEXT NOT NULL,
    handler_id TEXT NOT NULL,
    status TEXT NOT NULL
        CHECK (status IN ('success', 'nonzero', 'error', 'cancelled', 'deadline', 'unconfirmed')),
    exit_code INTEGER,
    resources_confirmed INTEGER NOT NULL CHECK (resources_confirmed IN (0, 1)),
    stdout_total INTEGER,
    stdout_retained INTEGER,
    stdout_truncated INTEGER,
    stderr_total INTEGER,
    stderr_retained INTEGER,
    stderr_truncated INTEGER,
    started_at INTEGER NOT NULL,
    finished_at INTEGER NOT NULL,
    primary_failure TEXT,
    PRIMARY KEY (sentinel_id, transition_id, handler_id),
    FOREIGN KEY (sentinel_id, transition_id, handler_id)
        REFERENCES sentinel_handler_results(sentinel_id, transition_id, handler_id),
    CHECK ((stdout_total IS NULL) = (stdout_retained IS NULL)
        AND (stdout_total IS NULL) = (stdout_truncated IS NULL)),
    CHECK ((stderr_total IS NULL) = (stderr_retained IS NULL)
        AND (stderr_total IS NULL) = (stderr_truncated IS NULL))
) STRICT;
CREATE TABLE sentinel_command_cleanup (
    sentinel_id TEXT NOT NULL,
    transition_id TEXT NOT NULL,
    handler_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    error TEXT NOT NULL,
    PRIMARY KEY (sentinel_id, transition_id, handler_id, ordinal),
    FOREIGN KEY (sentinel_id, transition_id, handler_id)
        REFERENCES sentinel_command_results(sentinel_id, transition_id, handler_id)
) STRICT;
"#;

/// Rebuilds `sentinel_definitions` with the generic `source_kind`/`source_spec` pair and
/// backfills existing probe/observe rows. It runs only for definitions created before that
/// pair existed (`definitions_carry_source_pair`), so no later source lane rebuilds the table.
pub(super) const SOURCE_SCHEMA: &str = r#"
PRAGMA defer_foreign_keys=ON;
CREATE TABLE sentinel_definitions_source_hold AS
    SELECT sentinel_id, version, provenance, name,
        CASE WHEN length(probe) > 0 THEN 'probe' ELSE 'observe' END AS source_kind,
        CAST(NULL AS TEXT) AS source_spec,
        probe, observe_kind, observe_ref, question, interval_ms, until_bucket,
        escalation_timeout_ms, max_transitions, max_actions, replaced_version,
        author_tab_id, cause_transition_id, authored_at, created_revision
    FROM sentinel_definitions WHERE 0;
INSERT INTO sentinel_definitions_source_hold
    (sentinel_id, version, provenance, name, source_kind, source_spec, probe,
     observe_kind, observe_ref, question, interval_ms, until_bucket, escalation_timeout_ms,
     max_transitions, max_actions, replaced_version, author_tab_id, cause_transition_id,
     authored_at, created_revision)
    SELECT sentinel_id, version, provenance, name,
        CASE WHEN length(probe) > 0 THEN 'probe' ELSE 'observe' END,
        NULL,
        probe, observe_kind, observe_ref, question, interval_ms, until_bucket,
        escalation_timeout_ms, max_transitions, max_actions, replaced_version,
        author_tab_id, cause_transition_id, authored_at, created_revision
    FROM sentinel_definitions;
DROP TABLE sentinel_definitions;
CREATE TABLE sentinel_definitions (
    sentinel_id TEXT NOT NULL REFERENCES sentinels(id),
    version INTEGER NOT NULL CHECK (version > 0),
    provenance TEXT NOT NULL CHECK (provenance IN ('authored', 'imported')),
    name TEXT,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('probe', 'observe', 'input', 'event')),
    source_spec TEXT,
    probe TEXT NOT NULL,
    observe_kind TEXT CHECK (observe_kind IN ('tab', 'cron_run')),
    observe_ref TEXT CHECK (observe_ref IS NULL OR length(observe_ref) > 0),
    question TEXT NOT NULL,
    interval_ms INTEGER NOT NULL CHECK (interval_ms > 0),
    until_bucket TEXT,
    escalation_timeout_ms INTEGER
        CHECK (escalation_timeout_ms IS NULL OR escalation_timeout_ms > 0),
    max_transitions INTEGER NOT NULL CHECK (max_transitions BETWEEN 1 AND 65535),
    max_actions INTEGER NOT NULL CHECK (max_actions BETWEEN 1 AND 65535),
    replaced_version INTEGER,
    author_tab_id TEXT CHECK (author_tab_id IS NULL OR length(author_tab_id) > 0),
    cause_transition_id TEXT
        CHECK (cause_transition_id IS NULL OR length(cause_transition_id) > 0),
    authored_at INTEGER CHECK (authored_at IS NULL OR authored_at >= 0),
    created_revision INTEGER NOT NULL CHECK (created_revision > 0),
    PRIMARY KEY (sentinel_id, version),
    CHECK ((source_kind = 'probe') = (length(probe) > 0)),
    CHECK ((source_kind = 'observe') = (observe_kind IS NOT NULL)),
    CHECK ((source_kind IN ('input', 'event')) = (source_spec IS NOT NULL)),
    CHECK ((observe_kind IS NULL) = (observe_ref IS NULL)),
    CHECK (replaced_version IS NULL OR (replaced_version > 0 AND replaced_version < version)),
    CHECK ((replaced_version IS NULL) = (author_tab_id IS NULL)
        AND (replaced_version IS NULL) = (authored_at IS NULL)),
    CHECK (cause_transition_id IS NULL OR replaced_version IS NOT NULL)
) STRICT;
INSERT INTO sentinel_definitions
    (sentinel_id, version, provenance, name, source_kind, source_spec, probe,
     observe_kind, observe_ref, question, interval_ms, until_bucket, escalation_timeout_ms,
     max_transitions, max_actions, replaced_version, author_tab_id, cause_transition_id,
     authored_at, created_revision)
    SELECT sentinel_id, version, provenance, name, source_kind, source_spec, probe,
     observe_kind, observe_ref, question, interval_ms, until_bucket, escalation_timeout_ms,
     max_transitions, max_actions, replaced_version, author_tab_id, cause_transition_id,
     authored_at, created_revision
    FROM sentinel_definitions_source_hold;
DROP TABLE sentinel_definitions_source_hold;
"#;

pub(super) const PIPELINE_SCHEMA: &str = r#"
ALTER TABLE sentinel_definitions ADD COLUMN pipeline_spec TEXT;
CREATE TABLE blueprint_step_journal (
    entry_id INTEGER PRIMARY KEY AUTOINCREMENT,
    sentinel_id TEXT NOT NULL REFERENCES sentinels(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL CHECK (length(run_id) BETWEEN 1 AND 128),
    step_index INTEGER NOT NULL CHECK (step_index >= -1),
    step_name TEXT NOT NULL CHECK (length(step_name) BETWEEN 1 AND 128),
    state TEXT NOT NULL CHECK (state IN ('running', 'done', 'failed', 'skipped')),
    outcome TEXT CHECK (outcome IS NULL OR length(outcome) BETWEEN 1 AND 128),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    ended_at_ms INTEGER CHECK (ended_at_ms IS NULL OR ended_at_ms >= started_at_ms),
    result_summary TEXT CHECK (result_summary IS NULL OR length(result_summary) <= 200),
    reason TEXT CHECK (reason IS NULL OR length(reason) <= 200),
    CHECK ((state = 'running') = (ended_at_ms IS NULL))
) STRICT;
CREATE INDEX blueprint_step_journal_newest
    ON blueprint_step_journal(sentinel_id, entry_id DESC);
"#;

/// Whether `sentinel_definitions` already carries the source pair, so `SOURCE_SCHEMA` is a no-op.
pub(super) fn definitions_carry_source_pair(conn: &Connection) -> Result<bool, StorageError> {
    let columns: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sentinel_definitions') \
             WHERE name = 'source_kind'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_error("inspect sentinel definitions", e))?;
    Ok(columns > 0)
}

/// The graph rebuild drops the Sentinel tables, so it runs only while they hold no stage.
pub(super) fn refuse_stored_stages(conn: &Connection) -> Result<(), StorageError> {
    let stored: bool = conn
        .query_row("SELECT EXISTS (SELECT 1 FROM sentinels)", [], |row| {
            row.get(0)
        })
        .map_err(|e| sqlite_error("check stored sentinels", e))?;
    if !stored {
        return Ok(());
    }
    let schema_version = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| sqlite_error("read schema version", e))?;
    Err(StorageError::MigrationRefused {
        schema_version,
        reason: "stored Sentinel stages would be discarded by the Sentinel graph rebuild",
    })
}

#[cfg(any(test, feature = "test-support"))]
mod support {
    use crate::assets::{self, Layout};
    use crate::error::{io_error, sqlite_error, StorageError};
    use crate::owner::Storage;
    use rusqlite::OpenFlags;
    use std::path::Path;

    impl Storage {
        /// Creates a root whose database stops at an earlier schema version, so contract
        /// tests can prove the registered migrations upgrade it in place.
        pub fn create_at_schema_version(root: &Path, version: i32) -> Result<(), StorageError> {
            let migrations = super::super::MIGRATIONS;
            let count = usize::try_from(version)
                .ok()
                .filter(|count| (1..migrations.len()).contains(count))
                .ok_or_else(|| {
                    StorageError::invalid("schema version", "must name an earlier migration")
                })?;
            std::fs::create_dir_all(root).map_err(|e| io_error("create instance root", e))?;
            let layout = Layout::of_root(&assets::canonical_dir(root)?);
            layout.create_dirs(true)?;
            if assets::entry_exists(&layout.database)? {
                return Err(StorageError::InconsistentRoot {
                    root: root.to_path_buf(),
                    reason: "database already exists",
                });
            }
            let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
            let conn = super::super::open_existing(&layout.database, flags)?;
            let mode: String = conn
                .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
                .map_err(|e| sqlite_error("enable wal", e))?;
            if !mode.eq_ignore_ascii_case("wal") {
                return Err(StorageError::Settings {
                    setting: "journal_mode",
                    expected: "wal".to_string(),
                    actual: mode,
                });
            }
            conn.execute_batch(&migrations[..count].concat())
                .map_err(|e| sqlite_error("apply migration", e))?;
            conn.pragma_update(None, "application_id", super::super::APPLICATION_ID)
                .map_err(|e| sqlite_error("stamp application id", e))?;
            conn.pragma_update(None, "user_version", version)
                .map_err(|e| sqlite_error("stamp schema version", e))?;
            conn.close()
                .map_err(|(_, e)| sqlite_error("close database", e))
        }
    }
}
