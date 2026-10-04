//! Stage runtime, transitions and handler results. Every action-authorizing operation
//! re-asserts the persisted lease predicate inside its own write transaction.

use super::definitions;
use super::queries;
use super::records::{
    BatchFence, CommandResultRecord, CommandStatus, DefinitionProvenance, Fence,
    HandlerResultRecord, HandlerStatus, MutationOutcome, NewTransition, RuntimeOutcome,
    ScheduleUpdate, SentinelCommit, SentinelCreate, SentinelId, SentinelIdentity, SentinelImport,
    StageDefinition, StageRuntime, TransitionRecord, MAX_SENTINEL_RECORDS,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{from_sql_u64, to_sql_u64};
use rusqlite::{params, Connection, OptionalExtension};

/// Child tables in an order that satisfies every foreign key while deleting.
const OWNED_TABLES: &[&str] = &[
    "sentinel_command_cleanup",
    "sentinel_command_results",
    "sentinel_handler_results",
    "sentinel_transitions",
    "sentinel_handler_targets",
    "sentinel_handlers",
    "sentinel_buckets",
    "sentinel_heads",
    "sentinel_definitions",
    "sentinel_runtime",
];

pub(super) fn handler_status_text(status: HandlerStatus) -> &'static str {
    match status {
        HandlerStatus::Unconfirmed => "unconfirmed",
        HandlerStatus::Completed => "completed",
        HandlerStatus::Failed => "failed",
        HandlerStatus::Noop => "noop",
        HandlerStatus::Skipped => "skipped",
    }
}

pub(super) fn handler_status_from(text: &str) -> Result<HandlerStatus, StorageError> {
    Ok(match text {
        "unconfirmed" => HandlerStatus::Unconfirmed,
        "completed" => HandlerStatus::Completed,
        "failed" => HandlerStatus::Failed,
        "noop" => HandlerStatus::Noop,
        "skipped" => HandlerStatus::Skipped,
        _ => return Err(corrupt("unknown handler status")),
    })
}

pub(super) fn command_status_text(status: CommandStatus) -> &'static str {
    match status {
        CommandStatus::Success => "success",
        CommandStatus::Nonzero => "nonzero",
        CommandStatus::Error => "error",
        CommandStatus::Cancelled => "cancelled",
        CommandStatus::Deadline => "deadline",
        CommandStatus::Unconfirmed => "unconfirmed",
    }
}

pub(super) fn command_status_from(text: &str) -> Result<CommandStatus, StorageError> {
    Ok(match text {
        "success" => CommandStatus::Success,
        "nonzero" => CommandStatus::Nonzero,
        "error" => CommandStatus::Error,
        "cancelled" => CommandStatus::Cancelled,
        "deadline" => CommandStatus::Deadline,
        "unconfirmed" => CommandStatus::Unconfirmed,
        _ => return Err(corrupt("unknown command status")),
    })
}

pub(super) fn corrupt(detail: &str) -> StorageError {
    StorageError::Corrupt {
        detail: format!("sentinel runtime: {detail}"),
    }
}

struct Live {
    generation: u64,
    paused: bool,
    stopped: bool,
    last_hash: Option<String>,
    last_bucket: Option<String>,
}

fn live(conn: &Connection, id: &str) -> Result<Option<Live>, StorageError> {
    let row = conn
        .prepare_cached(
            "SELECT generation, paused, stopped, last_hash, last_bucket \
             FROM sentinel_runtime WHERE sentinel_id = ?1",
        )
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                })
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel runtime", e))?;
    let Some((generation, paused, stopped, last_hash, last_bucket)) = row else {
        return Ok(None);
    };
    Ok(Some(Live {
        generation: from_sql_u64("generation", generation)?,
        paused,
        stopped,
        last_hash,
        last_bucket,
    }))
}

fn scalar(conn: &Connection, sql: &str, id: &str) -> Result<i64, StorageError> {
    conn.prepare_cached(sql)
        .and_then(|mut statement| statement.query_row([id], |row| row.get(0)))
        .map_err(|e| sqlite_error("read sentinel counter", e))
}

fn exists(conn: &Connection, id: &str) -> Result<bool, StorageError> {
    Ok(scalar(conn, "SELECT COUNT(*) FROM sentinels WHERE id = ?1", id)? > 0)
}

fn record_count(conn: &Connection) -> Result<usize, StorageError> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sentinels", [], |row| row.get(0))
        .map_err(|e| sqlite_error("count sentinels", e))?;
    usize::try_from(count).map_err(|_| corrupt("negative sentinel count"))
}

/// `(transition_id, terminal)` of the newest accepted transition.
fn last_transition(conn: &Connection, id: &str) -> Result<Option<(String, bool)>, StorageError> {
    conn.prepare_cached(
        "SELECT transition_id, terminal FROM sentinel_transitions WHERE sentinel_id = ?1 \
         ORDER BY ordinal DESC LIMIT 1",
    )
    .and_then(|mut statement| {
        statement
            .query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()
    })
    .map_err(|e| sqlite_error("read sentinel transition", e))
}

fn transition_bucket(
    conn: &Connection,
    id: &str,
    transition: &str,
) -> Result<Option<String>, StorageError> {
    conn.prepare_cached(
        "SELECT bucket FROM sentinel_transitions WHERE sentinel_id = ?1 AND transition_id = ?2",
    )
    .and_then(|mut statement| {
        statement
            .query_row(params![id, transition], |row| row.get(0))
            .optional()
    })
    .map_err(|e| sqlite_error("read sentinel transition", e))
}

fn result_exists(
    conn: &Connection,
    id: &str,
    transition: &str,
    handler: &str,
) -> Result<bool, StorageError> {
    let found: Option<i64> = conn
        .prepare_cached(
            "SELECT ordinal FROM sentinel_handler_results \
             WHERE sentinel_id = ?1 AND transition_id = ?2 AND handler_id = ?3",
        )
        .and_then(|mut statement| {
            statement
                .query_row(params![id, transition, handler], |row| row.get(0))
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel result", e))?;
    Ok(found.is_some())
}

fn next_result_ordinal(conn: &Connection, id: &str, transition: &str) -> Result<i64, StorageError> {
    conn.prepare_cached(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM sentinel_handler_results \
         WHERE sentinel_id = ?1 AND transition_id = ?2",
    )
    .and_then(|mut statement| statement.query_row(params![id, transition], |row| row.get(0)))
    .map_err(|e| sqlite_error("read sentinel result order", e))
}

/// Records which commit last changed any part of this stage's aggregate.
fn touch(cx: &mut WriteCx<'_>, id: &str) -> Result<(), StorageError> {
    let revision = to_sql_u64("revision", cx.revision.0)?;
    cx.write(
        "UPDATE sentinels SET revision = ?2 WHERE id = ?1",
        params![id, revision],
    )?;
    Ok(())
}

fn current(
    conn: &Connection,
    id: &str,
) -> Result<Option<super::records::StoredSentinel>, StorageError> {
    queries::load_one(conn, id)
}

fn fenced(conn: &Connection, fence: Fence, id: &str) -> Result<SentinelCommit, StorageError> {
    Ok(SentinelCommit::fenced(fence, current(conn, id)?))
}

fn finished(
    conn: &Connection,
    outcome: MutationOutcome,
    ids: &[String],
) -> Result<SentinelCommit, StorageError> {
    let mut sentinels = Vec::with_capacity(ids.len());
    for (position, id) in ids.iter().enumerate() {
        if ids[..position].contains(id) {
            continue;
        }
        sentinels.push(current(conn, id)?.ok_or_else(|| corrupt("touched stage vanished"))?);
    }
    Ok(SentinelCommit {
        outcome,
        sentinels,
        removed: Vec::new(),
    })
}

fn insert_identity(
    cx: &mut WriteCx<'_>,
    identity: &SentinelIdentity,
    scenario: Option<&SentinelId>,
) -> Result<(), StorageError> {
    let revision = to_sql_u64("revision", cx.revision.0)?;
    cx.write(
        "INSERT INTO sentinels (id, ordinal, creator_tab_id, workspace_id, team_id, cwd, \
         agent_authored, scenario_id, revision) \
         VALUES (?1, (SELECT COALESCE(MAX(ordinal), -1) + 1 FROM sentinels), \
         ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            identity.id.as_str(),
            identity.scope.creator_tab_id,
            identity.scope.workspace_id,
            identity.scope.team_id,
            identity.cwd,
            identity.agent_authored,
            scenario.map(SentinelId::as_str),
            revision
        ],
    )?;
    Ok(())
}

fn insert_runtime(
    cx: &mut WriteCx<'_>,
    id: &str,
    runtime: &StageRuntime,
) -> Result<(), StorageError> {
    let expires_at = runtime
        .expires_at
        .map(|at| to_sql_u64("runtime expiry", at))
        .transpose()?;
    cx.write(
        "INSERT INTO sentinel_runtime (sentinel_id, generation, next_run, expires_at, paused, \
         stopped, last_hash, last_bucket, last_reason) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            to_sql_u64("generation", runtime.generation)?,
            to_sql_u64("next run", runtime.next_run)?,
            expires_at,
            runtime.paused,
            runtime.stopped,
            runtime.last_hash,
            runtime.last_bucket,
            runtime.last_reason
        ],
    )?;
    Ok(())
}

fn write_outcome(
    cx: &mut WriteCx<'_>,
    id: &str,
    outcome: &RuntimeOutcome,
) -> Result<(), StorageError> {
    cx.write(
        "UPDATE sentinel_runtime SET last_hash = ?2, last_bucket = ?3, stopped = ?4, \
         last_reason = ?5 WHERE sentinel_id = ?1",
        params![
            id,
            outcome.last_hash,
            outcome.last_bucket,
            outcome.stopped,
            outcome.last_reason
        ],
    )?;
    Ok(())
}

fn insert_transition(
    cx: &mut WriteCx<'_>,
    id: &str,
    ordinal: i64,
    transition: &NewTransition,
    version: Option<u64>,
) -> Result<(), StorageError> {
    let version = version
        .map(|version| to_sql_u64("definition version", version))
        .transpose()?;
    cx.write(
        "INSERT INTO sentinel_transitions (sentinel_id, ordinal, transition_id, bucket, \
         accepted_at, terminal, definition_version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            ordinal,
            transition.id,
            transition.bucket,
            to_sql_u64("transition time", transition.accepted_at)?,
            transition.terminal,
            version
        ],
    )?;
    Ok(())
}

fn insert_result(
    cx: &mut WriteCx<'_>,
    id: &str,
    transition: &str,
    ordinal: i64,
    record: &HandlerResultRecord,
) -> Result<(), StorageError> {
    let accepted_at = record
        .accepted_at
        .map(|at| to_sql_u64("claim time", at))
        .transpose()?;
    cx.write(
        "INSERT INTO sentinel_handler_results (sentinel_id, transition_id, ordinal, handler_id, \
         status, claim_id, execution_id, accepted_at, reason) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            transition,
            ordinal,
            record.handler_id,
            handler_status_text(record.status),
            record.claim_id,
            record.execution_id,
            accepted_at,
            record.reason
        ],
    )?;
    if let Some(command) = &record.command_result {
        insert_command(cx, id, transition, &record.handler_id, command)?;
    }
    Ok(())
}

type CaptureColumns = (Option<i64>, Option<i64>, Option<bool>);

fn capture_columns(
    capture: Option<&super::records::CaptureRecord>,
) -> Result<CaptureColumns, StorageError> {
    let Some(capture) = capture else {
        return Ok((None, None, None));
    };
    Ok((
        Some(to_sql_u64("capture total", capture.total_bytes)?),
        Some(to_sql_u64("capture retained", capture.retained_bytes)?),
        Some(capture.truncated),
    ))
}

fn insert_command(
    cx: &mut WriteCx<'_>,
    id: &str,
    transition: &str,
    handler: &str,
    command: &CommandResultRecord,
) -> Result<(), StorageError> {
    let (out_total, out_retained, out_truncated) = capture_columns(command.stdout.as_ref())?;
    let (err_total, err_retained, err_truncated) = capture_columns(command.stderr.as_ref())?;
    cx.write(
        "INSERT INTO sentinel_command_results (sentinel_id, transition_id, handler_id, status, \
         exit_code, resources_confirmed, stdout_total, stdout_retained, stdout_truncated, \
         stderr_total, stderr_retained, stderr_truncated, started_at, finished_at, \
         primary_failure) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            id,
            transition,
            handler,
            command_status_text(command.status),
            command.exit_code,
            command.resources_confirmed,
            out_total,
            out_retained,
            out_truncated,
            err_total,
            err_retained,
            err_truncated,
            to_sql_u64("command started", command.started_at)?,
            to_sql_u64("command finished", command.finished_at)?,
            command.primary
        ],
    )?;
    for (ordinal, error) in (0_i64..).zip(&command.cleanup) {
        cx.write(
            "INSERT INTO sentinel_command_cleanup (sentinel_id, transition_id, handler_id, \
             ordinal, error) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, transition, handler, ordinal, error],
        )?;
    }
    Ok(())
}

fn replace_command(
    cx: &mut WriteCx<'_>,
    id: &str,
    transition: &str,
    handler: &str,
    command: Option<&CommandResultRecord>,
) -> Result<(), StorageError> {
    for table in ["sentinel_command_cleanup", "sentinel_command_results"] {
        cx.write(
            &format!(
                "DELETE FROM {table} WHERE sentinel_id = ?1 AND transition_id = ?2 \
                 AND handler_id = ?3"
            ),
            params![id, transition, handler],
        )?;
    }
    if let Some(command) = command {
        insert_command(cx, id, transition, handler, command)?;
    }
    Ok(())
}

/// The persisted subset of batch liveness; `None` means the lease may still act.
fn check_batch(
    conn: &Connection,
    id: &str,
    fence: &BatchFence,
) -> Result<Option<Fence>, StorageError> {
    let Some(live) = live(conn, id)? else {
        return Ok(Some(Fence::Missing));
    };
    if live.generation != fence.generation {
        return Ok(Some(Fence::Generation));
    }
    if live.last_hash != fence.last_hash || live.last_bucket != fence.last_bucket {
        return Ok(Some(Fence::Observation));
    }
    if live.paused {
        return Ok(Some(Fence::Paused));
    }
    Ok(match last_transition(conn, id)? {
        Some((transition, terminal)) if transition == fence.transition_id => {
            (live.stopped && !terminal).then_some(Fence::Stopped)
        }
        _ => Some(Fence::Transition),
    })
}

pub(super) fn create(
    cx: &mut WriteCx<'_>,
    create: SentinelCreate,
) -> Result<SentinelCommit, StorageError> {
    let id = create.identity.id.as_str().to_string();
    if exists(cx.conn, &id)? {
        return Ok(SentinelCommit::fenced(Fence::Duplicate, None));
    }
    if record_count(cx.conn)? >= MAX_SENTINEL_RECORDS {
        return Ok(SentinelCommit::fenced(Fence::Capacity, None));
    }
    if let Some(scenario) = &create.scenario_id {
        if scenario.as_str() != id && !exists(cx.conn, scenario.as_str())? {
            return Ok(SentinelCommit::fenced(Fence::Missing, None));
        }
    }
    insert_identity(cx, &create.identity, create.scenario_id.as_ref())?;
    definitions::insert_version(
        cx,
        &id,
        1,
        DefinitionProvenance::Authored,
        &create.definition,
    )?;
    definitions::insert_head(cx, &id, 1)?;
    insert_runtime(cx, &id, &create.runtime)?;
    let mut touched = vec![id];
    if let Some(scenario) = &create.scenario_id {
        let scope = &create.identity.scope;
        let revision = to_sql_u64("revision", cx.revision.0)?;
        for child in &create.associate {
            let joined = cx.write(
                "UPDATE sentinels SET scenario_id = ?2, revision = ?3 WHERE id = ?1 \
                 AND scenario_id IS NULL AND creator_tab_id = ?4 AND workspace_id = ?5 \
                 AND team_id IS ?6",
                params![
                    child.as_str(),
                    scenario.as_str(),
                    revision,
                    scope.creator_tab_id,
                    scope.workspace_id,
                    scope.team_id
                ],
            )?;
            if joined == 1 {
                touched.push(child.as_str().to_string());
            }
        }
    }
    finished(cx.conn, MutationOutcome::Applied, &touched)
}

/// Transaction join for the all-domain importer: exact existing stages as `Imported` v1.
pub(crate) fn import(
    cx: &mut WriteCx<'_>,
    records: Vec<SentinelImport>,
) -> Result<SentinelCommit, StorageError> {
    for record in &records {
        if exists(cx.conn, record.identity.id.as_str())? {
            return Ok(SentinelCommit::fenced(Fence::Duplicate, None));
        }
    }
    if record_count(cx.conn)? + records.len() > MAX_SENTINEL_RECORDS {
        return Ok(SentinelCommit::fenced(Fence::Capacity, None));
    }
    for record in &records {
        let id = record.identity.id.as_str();
        insert_identity(cx, &record.identity, None)?;
        definitions::insert_version(
            cx,
            id,
            1,
            DefinitionProvenance::Imported,
            &record.definition,
        )?;
        definitions::insert_head(cx, id, 1)?;
        insert_runtime(cx, id, &record.runtime)?;
        for (ordinal, transition) in (0_i64..).zip(&record.transitions) {
            import_transition(cx, id, ordinal, transition)?;
        }
    }
    let revision = to_sql_u64("revision", cx.revision.0)?;
    for record in &records {
        if let Some(scenario) = &record.scenario_id {
            cx.write(
                "UPDATE sentinels SET scenario_id = ?2, revision = ?3 WHERE id = ?1",
                params![record.identity.id.as_str(), scenario.as_str(), revision],
            )?;
        }
    }
    let ids: Vec<String> = records
        .iter()
        .map(|record| record.identity.id.as_str().to_string())
        .collect();
    finished(cx.conn, MutationOutcome::Applied, &ids)
}

fn import_transition(
    cx: &mut WriteCx<'_>,
    id: &str,
    ordinal: i64,
    transition: &TransitionRecord,
) -> Result<(), StorageError> {
    let accepted = NewTransition {
        id: transition.id.clone(),
        bucket: transition.bucket.clone(),
        accepted_at: transition.accepted_at,
        terminal: transition.terminal,
    };
    insert_transition(cx, id, ordinal, &accepted, None)?;
    for (position, result) in (0_i64..).zip(&transition.handlers) {
        insert_result(cx, id, &transition.id, position, result)?;
    }
    Ok(())
}

/// Appends an immutable version and moves the head; runtime and results stay untouched.
pub(super) fn update_definition(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    expect_version: u64,
    definition: StageDefinition,
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    let Some(head) = definitions::head_version(cx.conn, id)? else {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    };
    if head != expect_version {
        return fenced(cx.conn, Fence::Version, id);
    }
    let next = definitions::latest_version(cx.conn, id)? + 1;
    definitions::insert_version(cx, id, next, DefinitionProvenance::Authored, &definition)?;
    definitions::move_head(cx, id, next)?;
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}

fn stop_row(cx: &mut WriteCx<'_>, id: &str, reason: &str) -> Result<bool, StorageError> {
    let changed = cx.write(
        "UPDATE sentinel_runtime SET stopped = 1, generation = generation + 1, \
         last_reason = ?2 WHERE sentinel_id = ?1",
        params![id, reason],
    )?;
    if changed == 1 {
        touch(cx, id)?;
    }
    Ok(changed == 1)
}

pub(super) fn stop(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    reason: &str,
) -> Result<SentinelCommit, StorageError> {
    if !stop_row(cx, id.as_str(), reason)? {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    }
    finished(
        cx.conn,
        MutationOutcome::Applied,
        &[id.as_str().to_string()],
    )
}

/// Transaction join for Team teardown: deletes every row of the Team's stages and returns
/// their identities in durable order. Callers pass their own open write context.
pub(crate) fn remove_team_rows(
    cx: &mut WriteCx<'_>,
    team: &str,
) -> Result<Vec<SentinelId>, StorageError> {
    let ids: Vec<String> = {
        let mut statement = cx
            .conn
            .prepare_cached("SELECT id FROM sentinels WHERE team_id = ?1 ORDER BY ordinal")
            .map_err(|e| sqlite_error("read Team sentinels", e))?;
        let rows = statement
            .query_map([team], |row| row.get(0))
            .map_err(|e| sqlite_error("read Team sentinels", e))?;
        rows.collect::<Result<_, _>>()
            .map_err(|e| sqlite_error("read Team sentinels", e))?
    };
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    for table in OWNED_TABLES {
        cx.write(
            &format!(
                "DELETE FROM {table} WHERE sentinel_id IN \
                 (SELECT id FROM sentinels WHERE team_id = ?1)"
            ),
            [team],
        )?;
    }
    cx.write("DELETE FROM sentinels WHERE team_id = ?1", [team])?;
    Ok(ids.into_iter().map(SentinelId::trusted).collect())
}

pub(super) fn remove_team(
    cx: &mut WriteCx<'_>,
    team: &str,
) -> Result<SentinelCommit, StorageError> {
    let removed = remove_team_rows(cx, team)?;
    Ok(SentinelCommit {
        outcome: if removed.is_empty() {
            MutationOutcome::Unchanged
        } else {
            MutationOutcome::Applied
        },
        sentinels: Vec::new(),
        removed,
    })
}

/// Scheduling never authors a definition; stages removed meanwhile are skipped.
pub(super) fn schedule(
    cx: &mut WriteCx<'_>,
    updates: Vec<ScheduleUpdate>,
) -> Result<SentinelCommit, StorageError> {
    let mut touched = Vec::new();
    for update in updates {
        match update {
            ScheduleUpdate::Advance { id, next_run } => {
                let changed = cx.write(
                    "UPDATE sentinel_runtime SET next_run = ?2 WHERE sentinel_id = ?1",
                    params![id.as_str(), to_sql_u64("next run", next_run)?],
                )?;
                if changed == 1 {
                    touch(cx, id.as_str())?;
                    touched.push(id.as_str().to_string());
                }
            }
            ScheduleUpdate::Stop { id, reason } => {
                if stop_row(cx, id.as_str(), &reason)? {
                    touched.push(id.as_str().to_string());
                }
            }
        }
    }
    let outcome = if touched.is_empty() {
        MutationOutcome::Unchanged
    } else {
        MutationOutcome::Applied
    };
    finished(cx.conn, outcome, &touched)
}

pub(super) fn finish(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    generation: u64,
    outcome: &RuntimeOutcome,
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    let Some(live) = live(cx.conn, id)? else {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    };
    if live.generation != generation {
        return fenced(cx.conn, Fence::Generation, id);
    }
    if live.stopped {
        return fenced(cx.conn, Fence::Stopped, id);
    }
    write_outcome(cx, id, outcome)?;
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}

pub(super) fn accept_transition(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    generation: u64,
    outcome: &RuntimeOutcome,
    transition: &NewTransition,
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    let Some(live) = live(cx.conn, id)? else {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    };
    let fence = if transition_bucket(cx.conn, id, &transition.id)?.is_some() {
        Some(Fence::Duplicate)
    } else if live.generation != generation {
        Some(Fence::Generation)
    } else if live.stopped {
        Some(Fence::Stopped)
    } else if live.paused {
        Some(Fence::Paused)
    } else {
        let accepted = scalar(
            cx.conn,
            "SELECT COUNT(*) FROM sentinel_transitions WHERE sentinel_id = ?1",
            id,
        )?;
        let (max_transitions, _) = definitions::head_budgets(cx.conn, id)?;
        (from_sql_u64("transition count", accepted)? >= max_transitions).then_some(Fence::Budget)
    };
    if let Some(fence) = fence {
        return fenced(cx.conn, fence, id);
    }
    let ordinal = scalar(
        cx.conn,
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM sentinel_transitions WHERE sentinel_id = ?1",
        id,
    )?;
    let version = definitions::head_version(cx.conn, id)?;
    insert_transition(cx, id, ordinal, transition, version)?;
    write_outcome(cx, id, outcome)?;
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}

pub(super) fn claim_handler(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    fence: &BatchFence,
    claim: &HandlerResultRecord,
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    if let Some(refused) = check_batch(cx.conn, id, fence)? {
        return fenced(cx.conn, refused, id);
    }
    let transition = fence.transition_id.as_str();
    if result_exists(cx.conn, id, transition, &claim.handler_id)? {
        return fenced(cx.conn, Fence::Claimed, id);
    }
    let bucket = transition_bucket(cx.conn, id, transition)?
        .ok_or_else(|| corrupt("fenced transition vanished"))?;
    if definitions::head_handler(cx.conn, id, &bucket, &claim.handler_id)?.is_none() {
        return fenced(cx.conn, Fence::Undeclared, id);
    }
    let claimed = scalar(
        cx.conn,
        "SELECT COUNT(*) FROM sentinel_handler_results \
         WHERE sentinel_id = ?1 AND claim_id IS NOT NULL",
        id,
    )?;
    let (_, max_actions) = definitions::head_budgets(cx.conn, id)?;
    if from_sql_u64("claim count", claimed)? >= max_actions {
        return fenced(cx.conn, Fence::Budget, id);
    }
    let ordinal = next_result_ordinal(cx.conn, id, transition)?;
    insert_result(cx, id, transition, ordinal, claim)?;
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}

/// Records the effect of an existing durable claim; never fenced by later stops.
pub(super) fn finish_handler_result(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    transition: &str,
    result: &HandlerResultRecord,
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    let found: Option<i64> = cx
        .conn
        .prepare_cached(
            "SELECT ordinal FROM sentinel_handler_results WHERE sentinel_id = ?1 \
             AND transition_id = ?2 AND handler_id = ?3 AND claim_id IS ?4",
        )
        .and_then(|mut statement| {
            statement
                .query_row(
                    params![id, transition, result.handler_id, result.claim_id],
                    |row| row.get(0),
                )
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel claim", e))?;
    if found.is_none() {
        return fenced(cx.conn, Fence::NotClaimed, id);
    }
    cx.write(
        "UPDATE sentinel_handler_results SET status = ?4, execution_id = ?5, reason = ?6 \
         WHERE sentinel_id = ?1 AND transition_id = ?2 AND handler_id = ?3",
        params![
            id,
            transition,
            result.handler_id,
            handler_status_text(result.status),
            result.execution_id,
            result.reason
        ],
    )?;
    replace_command(
        cx,
        id,
        transition,
        &result.handler_id,
        result.command_result.as_ref(),
    )?;
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}

pub(super) fn skip_handlers(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    transition: &str,
    skipped: &[HandlerResultRecord],
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    if transition_bucket(cx.conn, id, transition)?.is_none() {
        return fenced(cx.conn, Fence::Missing, id);
    }
    let mut inserted = false;
    for record in skipped {
        if result_exists(cx.conn, id, transition, &record.handler_id)? {
            continue;
        }
        let ordinal = next_result_ordinal(cx.conn, id, transition)?;
        insert_result(cx, id, transition, ordinal, record)?;
        inserted = true;
    }
    if !inserted {
        return finished(cx.conn, MutationOutcome::Unchanged, &[id.to_string()]);
    }
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}

type ScopeRow = (String, String, Option<String>);

fn scope_row(conn: &Connection, id: &str) -> Result<Option<ScopeRow>, StorageError> {
    conn.prepare_cached("SELECT creator_tab_id, workspace_id, team_id FROM sentinels WHERE id = ?1")
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel scope", e))
}

/// Unpauses a paused target exactly once on behalf of a durable declared claim.
pub(super) fn activate_target(
    cx: &mut WriteCx<'_>,
    source: &SentinelId,
    fence: &BatchFence,
    target: &SentinelId,
    active_limit: usize,
) -> Result<SentinelCommit, StorageError> {
    let (source, target) = (source.as_str(), target.as_str());
    if let Some(refused) = check_batch(cx.conn, source, fence)? {
        return fenced(cx.conn, refused, source);
    }
    let claimed: i64 = cx
        .conn
        .prepare_cached(
            "SELECT COUNT(*) FROM sentinel_handler_results r \
             JOIN sentinel_transitions t \
               ON t.sentinel_id = r.sentinel_id AND t.transition_id = r.transition_id \
             JOIN sentinel_heads h ON h.sentinel_id = r.sentinel_id \
             JOIN sentinel_handlers hd ON hd.sentinel_id = h.sentinel_id \
               AND hd.version = h.version AND hd.handler_id = r.handler_id \
               AND hd.bucket = t.bucket \
             WHERE r.sentinel_id = ?1 AND r.transition_id = ?2 AND hd.kind = 'sentinel' \
               AND hd.target_ref = ?3 AND r.claim_id IS NOT NULL AND r.status = 'unconfirmed'",
        )
        .and_then(|mut statement| {
            statement.query_row(params![source, fence.transition_id, target], |row| {
                row.get(0)
            })
        })
        .map_err(|e| sqlite_error("read activation claim", e))?;
    if claimed == 0 {
        return fenced(cx.conn, Fence::NotClaimed, source);
    }
    let Some(target_scope) = scope_row(cx.conn, target)? else {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    };
    if source == target || scope_row(cx.conn, source)? != Some(target_scope) {
        return fenced(cx.conn, Fence::Scope, target);
    }
    let Some(state) = live(cx.conn, target)? else {
        return Err(corrupt("stage has no runtime"));
    };
    if state.stopped {
        return fenced(cx.conn, Fence::Stopped, target);
    }
    if !state.paused {
        return finished(cx.conn, MutationOutcome::Unchanged, &[target.to_string()]);
    }
    let active: i64 = cx
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sentinel_runtime WHERE stopped = 0 AND paused = 0",
            [],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_error("count active sentinels", e))?;
    if usize::try_from(active).map_err(|_| corrupt("negative active count"))? >= active_limit {
        return fenced(cx.conn, Fence::Capacity, target);
    }
    cx.write(
        "UPDATE sentinel_runtime SET paused = 0, generation = generation + 1 WHERE sentinel_id = ?1",
        [target],
    )?;
    touch(cx, target)?;
    finished(cx.conn, MutationOutcome::Applied, &[target.to_string()])
}
