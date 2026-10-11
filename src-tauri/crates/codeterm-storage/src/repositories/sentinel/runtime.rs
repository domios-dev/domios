//! Stage runtime, transitions and handler results. Every action-authorizing operation
//! re-asserts the persisted lease predicate inside its own write transaction.

use super::definitions;
use super::queries;
use super::records::{
    AmendmentRecord, BatchFence, CommandResultRecord, CommandStatus, DefinitionProvenance,
    DefinitionUpdate, Fence, HandlerResultRecord, HandlerStatus, MutationOutcome, NewTransition,
    RunRecord, RuntimeOutcome, ScheduleUpdate, SentinelCommit, SentinelCreate, SentinelId,
    SentinelIdentity, SentinelImport, StageRuntime, TransitionRecord, MAX_SENTINEL_RECORDS,
    MAX_TRANSITIONS,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{from_sql_u64, to_sql_u64};
use rusqlite::{params, Connection, OptionalExtension};

mod activation;

pub(super) fn activate_target(
    cx: &mut WriteCx<'_>,
    source: &SentinelId,
    fence: &BatchFence,
    target: &SentinelId,
    active_limit: usize,
    input_due_at: Option<u64>,
) -> Result<SentinelCommit, StorageError> {
    activation::activate_target(cx, source, fence, target, active_limit, input_due_at)
}

/// Child tables in an order that satisfies every foreign key while deleting.
const OWNED_TABLES: &[&str] = &[
    "blueprint_command_cleanup",
    "blueprint_command_results",
    "blueprint_handler_results",
    "blueprint_transitions",
    "blueprint_handler_targets",
    "blueprint_handlers",
    "blueprint_buckets",
    "blueprint_heads",
    "blueprint_definitions",
    "blueprint_runtime",
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
        detail: format!("blueprint runtime: {detail}"),
    }
}

struct Live {
    generation: u64,
    paused: bool,
    stopped: bool,
    last_hash: Option<String>,
    last_bucket: Option<String>,
    run_id: Option<String>,
    run_tab_id: Option<String>,
    run_transitions: u16,
}

fn live(conn: &Connection, id: &str) -> Result<Option<Live>, StorageError> {
    let row = conn
        .prepare_cached(
            "SELECT generation, paused, stopped, last_hash, last_bucket, run_id, run_tab_id, run_transitions \
             FROM blueprint_runtime WHERE blueprint_id = ?1",
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
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, i64>(7)?,
                    ))
                })
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel runtime", e))?;
    let Some((
        generation,
        paused,
        stopped,
        last_hash,
        last_bucket,
        run_id,
        run_tab_id,
        run_transitions,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(Live {
        generation: from_sql_u64("generation", generation)?,
        paused,
        stopped,
        last_hash,
        last_bucket,
        run_id,
        run_tab_id,
        run_transitions: u16::try_from(from_sql_u64("run transition count", run_transitions)?)
            .map_err(|_| corrupt("run transition count exceeds its bound"))?,
    }))
}

fn observes_cron_run(conn: &Connection, id: &str, cron_id: &str) -> Result<bool, StorageError> {
    let observed = conn
        .prepare_cached(
            "SELECT d.observe_kind, d.observe_ref FROM blueprint_heads h \
             JOIN blueprint_definitions d ON d.blueprint_id = h.blueprint_id \
             AND d.version = h.version WHERE h.blueprint_id = ?1",
        )
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                })
                .optional()
        })
        .map_err(|e| sqlite_error("read sentinel run source", e))?;
    Ok(observed.is_some_and(|(kind, reference)| {
        kind.as_deref() == Some("cron_run") && reference.as_deref() == Some(cron_id)
    }))
}

fn run_identity_matches(live: &Live, expected: Option<&RunRecord>) -> bool {
    match (live.run_id.as_deref(), live.run_tab_id.as_deref(), expected) {
        (None, None, None) => true,
        (Some(run_id), Some(tab_id), Some(expected)) => {
            run_id == expected.run_id && tab_id == expected.tab_id
        }
        _ => false,
    }
}

fn scalar(conn: &Connection, sql: &str, id: &str) -> Result<i64, StorageError> {
    conn.prepare_cached(sql)
        .and_then(|mut statement| statement.query_row([id], |row| row.get(0)))
        .map_err(|e| sqlite_error("read sentinel counter", e))
}

fn exists(conn: &Connection, id: &str) -> Result<bool, StorageError> {
    Ok(scalar(conn, "SELECT COUNT(*) FROM blueprints WHERE id = ?1", id)? > 0)
}

fn record_count(conn: &Connection) -> Result<usize, StorageError> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM blueprints", [], |row| row.get(0))
        .map_err(|e| sqlite_error("count blueprints", e))?;
    usize::try_from(count).map_err(|_| corrupt("negative sentinel count"))
}

/// `(transition_id, terminal)` of the newest accepted transition.
fn last_transition(conn: &Connection, id: &str) -> Result<Option<(String, bool)>, StorageError> {
    conn.prepare_cached(
        "SELECT transition_id, terminal FROM blueprint_transitions WHERE blueprint_id = ?1 \
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
        "SELECT bucket FROM blueprint_transitions WHERE blueprint_id = ?1 AND transition_id = ?2",
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
            "SELECT ordinal FROM blueprint_handler_results \
             WHERE blueprint_id = ?1 AND transition_id = ?2 AND handler_id = ?3",
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
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM blueprint_handler_results \
         WHERE blueprint_id = ?1 AND transition_id = ?2",
    )
    .and_then(|mut statement| statement.query_row(params![id, transition], |row| row.get(0)))
    .map_err(|e| sqlite_error("read sentinel result order", e))
}

/// Records which commit last changed any part of this stage's aggregate.
fn touch(cx: &mut WriteCx<'_>, id: &str) -> Result<(), StorageError> {
    let revision = to_sql_u64("revision", cx.revision.0)?;
    cx.write(
        "UPDATE blueprints SET revision = ?2 WHERE id = ?1",
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
        "INSERT INTO blueprints (id, ordinal, creator_tab_id, workspace_id, team_id, cwd, \
         agent_authored, scenario_id, revision) \
         VALUES (?1, (SELECT COALESCE(MAX(ordinal), -1) + 1 FROM blueprints), \
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
    let run = runtime.run.as_ref();
    let bound_at = run
        .map(|run| to_sql_u64("run bound time", run.bound_at))
        .transpose()?;
    cx.write(
        "INSERT INTO blueprint_runtime (blueprint_id, generation, next_run, expires_at, paused, \
         stopped, last_hash, last_bucket, last_reason, held, run_id, run_tab_id, run_bound_at, run_transitions, causal_run_json, runs_since_transition, last_probe_at, probe_state) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            id,
            to_sql_u64("generation", runtime.generation)?,
            to_sql_u64("next run", runtime.next_run)?,
            expires_at,
            runtime.paused,
            runtime.stopped,
            runtime.last_hash,
            runtime.last_bucket,
            runtime.last_reason,
            runtime.held,
            run.map(|run| run.run_id.as_str()),
            run.map(|run| run.tab_id.as_str()),
            bound_at,
            runtime.run_transitions,
            runtime.causal_run_json,
            to_sql_u64("probe count", runtime.runs_since_transition)?,
            runtime.last_probe_at.map(|at| to_sql_u64("last probe", at)).transpose()?,
            runtime.probe_state
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
        "UPDATE blueprint_runtime SET last_hash = ?2, last_bucket = ?3, stopped = ?4, \
         last_reason = ?5, held = ?6, run_transitions = ?7, runs_since_transition = ?8, last_probe_at = ?9, probe_state = ?10 WHERE blueprint_id = ?1",
        params![
            id,
            outcome.last_hash,
            outcome.last_bucket,
            outcome.stopped,
            outcome.last_reason,
            outcome.held,
            outcome.run_transitions,
            to_sql_u64("probe count", outcome.runs_since_transition)?,
            outcome.last_probe_at.map(|at| to_sql_u64("last probe", at)).transpose()?,
            outcome.probe_state
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
        "INSERT INTO blueprint_transitions (blueprint_id, ordinal, transition_id, bucket, \
         accepted_at, terminal, definition_version, decision_json, causal_run_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            ordinal,
            transition.id,
            transition.bucket,
            to_sql_u64("transition time", transition.accepted_at)?,
            transition.terminal,
            version,
            transition.decision_json,
            transition.causal_run_json
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
        "INSERT INTO blueprint_handler_results (blueprint_id, transition_id, ordinal, handler_id, \
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
        "INSERT INTO blueprint_command_results (blueprint_id, transition_id, handler_id, status, \
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
            "INSERT INTO blueprint_command_cleanup (blueprint_id, transition_id, handler_id, \
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
    for table in ["blueprint_command_cleanup", "blueprint_command_results"] {
        cx.write(
            &format!(
                "DELETE FROM {table} WHERE blueprint_id = ?1 AND transition_id = ?2 \
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
        None,
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
                "UPDATE blueprints SET scenario_id = ?2, revision = ?3 WHERE id = ?1 \
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

/// Transaction join for the all-domain importer: each stage's observed head as `Imported`.
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
            record.version,
            DefinitionProvenance::Imported,
            record.amendment.as_ref(),
            &record.definition,
        )?;
        definitions::insert_head(cx, id, record.version)?;
        insert_runtime(cx, id, &record.runtime)?;
        for (ordinal, transition) in (0_i64..).zip(&record.transitions) {
            import_transition(cx, id, ordinal, transition)?;
        }
    }
    let revision = to_sql_u64("revision", cx.revision.0)?;
    for record in &records {
        if let Some(scenario) = &record.scenario_id {
            cx.write(
                "UPDATE blueprints SET scenario_id = ?2, revision = ?3 WHERE id = ?1",
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
        causal_run_json: transition.causal_run_json.clone(),
        decision_json: transition.decision_json.clone(),
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

/// Appends an immutable version with its lineage, moves the head and advances the generation,
/// so a lease taken under the replaced version can no longer act; nothing else changes.
pub(super) fn update_definition(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    expect_version: u64,
    update: DefinitionUpdate,
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    let Some(head) = definitions::head_version(cx.conn, id)? else {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    };
    if head != expect_version {
        return fenced(cx.conn, Fence::Version, id);
    }
    let next = definitions::latest_version(cx.conn, id)? + 1;
    let input_due_at = update
        .definition
        .pipeline()
        .is_some_and(|pipeline| {
            matches!(
                &pipeline.trigger,
                codeterm_types::blueprint_pipeline::Trigger::Input { .. }
            )
        })
        .then_some(update.authored_at);
    let amendment = AmendmentRecord {
        version: next,
        replaced_version: head,
        author_tab_id: update.author_tab_id,
        cause_transition_id: update.cause_transition_id,
        authored_at: update.authored_at,
    };
    definitions::insert_version(
        cx,
        id,
        next,
        DefinitionProvenance::Authored,
        Some(&amendment),
        &update.definition,
    )?;
    definitions::move_head(cx, id, next)?;
    let advanced = if let Some(next_run) = input_due_at {
        cx.write(
            "UPDATE blueprint_runtime SET generation = generation + 1, next_run = ?2 WHERE blueprint_id = ?1",
            params![id, to_sql_u64("input amendment due time", next_run)?],
        )?
    } else {
        cx.write(
            "UPDATE blueprint_runtime SET generation = generation + 1 WHERE blueprint_id = ?1",
            [id],
        )?
    };
    if advanced != 1 {
        return Err(corrupt("stage has no runtime"));
    }
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}

pub(super) fn set_run_binding(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    expected_generation: u64,
    cron_id: &str,
    expected_run: Option<&RunRecord>,
    run: Option<&RunRecord>,
    next_run: u64,
    last_reason: Option<&str>,
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    let Some(live) = live(cx.conn, id)? else {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    };
    if live.generation != expected_generation || !run_identity_matches(&live, expected_run) {
        return fenced(cx.conn, Fence::Generation, id);
    }
    if live.stopped && run.is_some() {
        return fenced(cx.conn, Fence::Stopped, id);
    }
    if !observes_cron_run(cx.conn, id, cron_id)? {
        return fenced(cx.conn, Fence::Observation, id);
    }
    if run.is_some_and(|next| {
        live.run_id.as_deref() == Some(next.run_id.as_str())
            && live.run_tab_id.as_deref() == Some(next.tab_id.as_str())
    }) {
        return fenced(cx.conn, Fence::Generation, id);
    }
    let generation = live
        .generation
        .checked_add(1)
        .ok_or_else(|| StorageError::invalid("run generation", "is exhausted"))?;
    let (run_id, run_tab_id, run_bound_at) = run.map_or((None, None, None), |run| {
        (
            Some(run.run_id.as_str()),
            Some(run.tab_id.as_str()),
            Some(run.bound_at),
        )
    });
    let changed = cx.write(
        "UPDATE blueprint_runtime SET generation = ?2, next_run = ?3, last_hash = NULL, \
         last_bucket = NULL, last_reason = ?4, held = NULL, run_id = ?5, run_tab_id = ?6, \
         run_bound_at = ?7, run_transitions = 0, runs_since_transition = 0, last_probe_at = NULL, probe_state = NULL WHERE blueprint_id = ?1 AND generation = ?8",
        params![
            id,
            to_sql_u64("run generation", generation)?,
            to_sql_u64("next run", next_run)?,
            last_reason,
            run_id,
            run_tab_id,
            run_bound_at
                .map(|at| to_sql_u64("run bound time", at))
                .transpose()?,
            to_sql_u64("expected run generation", expected_generation)?,
        ],
    )?;
    if changed != 1 {
        return fenced(cx.conn, Fence::Generation, id);
    }
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}

fn stop_row(cx: &mut WriteCx<'_>, id: &str, reason: &str) -> Result<bool, StorageError> {
    let changed = cx.write(
        "UPDATE blueprint_runtime SET stopped = 1, generation = generation + 1, \
         last_reason = ?2 WHERE blueprint_id = ?1",
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
            .prepare_cached("SELECT id FROM blueprints WHERE team_id = ?1 ORDER BY ordinal")
            .map_err(|e| sqlite_error("read Team blueprints", e))?;
        let rows = statement
            .query_map([team], |row| row.get(0))
            .map_err(|e| sqlite_error("read Team blueprints", e))?;
        rows.collect::<Result<_, _>>()
            .map_err(|e| sqlite_error("read Team blueprints", e))?
    };
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    for table in OWNED_TABLES {
        cx.write(
            &format!(
                "DELETE FROM {table} WHERE blueprint_id IN \
                 (SELECT id FROM blueprints WHERE team_id = ?1)"
            ),
            [team],
        )?;
    }
    cx.write("DELETE FROM blueprints WHERE team_id = ?1", [team])?;
    Ok(ids.into_iter().map(SentinelId::trusted).collect())
}

pub(super) fn delete_scenario(
    cx: &mut WriteCx<'_>,
    id: SentinelId,
    workspace: &str,
    team: Option<&str>,
) -> Result<SentinelCommit, StorageError> {
    let ids: Vec<String> = {
        let mut statement = cx
            .conn
            .prepare_cached(
                "SELECT id FROM blueprints WHERE workspace_id = ?1 AND team_id IS ?2 \
             AND (id = ?3 OR scenario_id = ?3) ORDER BY ordinal",
            )
            .map_err(|e| sqlite_error("read scenario blueprints", e))?;
        let rows = statement
            .query_map(params![workspace, team, id.as_str()], |row| row.get(0))
            .map_err(|e| sqlite_error("read scenario blueprints", e))?;
        rows.collect::<Result<_, _>>()
            .map_err(|e| sqlite_error("read scenario blueprints", e))?
    };
    if ids.is_empty() {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    }
    for table in OWNED_TABLES {
        cx.write(
            &format!(
                "DELETE FROM {table} WHERE blueprint_id IN \
                 (SELECT id FROM blueprints WHERE workspace_id = ?1 AND team_id IS ?2 \
                  AND (id = ?3 OR scenario_id = ?3))"
            ),
            params![workspace, team, id.as_str()],
        )?;
    }
    cx.write(
        "DELETE FROM blueprints WHERE workspace_id = ?1 AND team_id IS ?2 \
         AND (id = ?3 OR scenario_id = ?3)",
        params![workspace, team, id.as_str()],
    )?;
    Ok(SentinelCommit {
        outcome: MutationOutcome::Applied,
        sentinels: Vec::new(),
        removed: ids.into_iter().map(SentinelId::trusted).collect(),
    })
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
                    "UPDATE blueprint_runtime SET next_run = ?2 WHERE blueprint_id = ?1",
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
            "SELECT COUNT(*) FROM blueprint_transitions WHERE blueprint_id = ?1",
            id,
        )?;
        let (max_transitions, _) = definitions::head_budgets(cx.conn, id)?;
        let accepted = from_sql_u64("transition count", accepted)?;
        let per_run_spent = if live.run_id.is_some() {
            u64::from(live.run_transitions) >= max_transitions
        } else {
            accepted >= max_transitions
        };
        (per_run_spent || accepted >= MAX_TRANSITIONS as u64).then_some(Fence::Budget)
    };
    if let Some(fence) = fence {
        return fenced(cx.conn, fence, id);
    }
    let ordinal = scalar(
        cx.conn,
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM blueprint_transitions WHERE blueprint_id = ?1",
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
        "SELECT COUNT(*) FROM blueprint_handler_results \
         WHERE blueprint_id = ?1 AND claim_id IS NOT NULL",
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
            "SELECT ordinal FROM blueprint_handler_results WHERE blueprint_id = ?1 \
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
        "UPDATE blueprint_handler_results SET status = ?4, execution_id = ?5, reason = ?6 \
         WHERE blueprint_id = ?1 AND transition_id = ?2 AND handler_id = ?3",
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
