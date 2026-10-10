//! Coherent aggregate reads; callers run them inside one read or write transaction.

use super::definitions;
use super::records::{
    CaptureRecord, CommandResultRecord, HandlerResultRecord, RunRecord, SentinelId,
    SentinelIdentity, SentinelRecord, SentinelScope, StageRuntime, StoredSentinel,
    TransitionRecord, MAX_SENTINEL_RECORDS,
};
use super::runtime::{command_status_from, corrupt, handler_status_from};
use crate::error::{sqlite_error, StorageError};
use crate::repositories::{from_sql_u64, Revision};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

struct Header {
    ordinal: i64,
    creator_tab_id: String,
    workspace_id: String,
    team_id: Option<String>,
    cwd: String,
    agent_authored: bool,
    scenario_id: Option<String>,
    revision: i64,
    version: Option<i64>,
    generation: Option<i64>,
    next_run: Option<i64>,
    expires_at: Option<i64>,
    paused: Option<bool>,
    stopped: Option<bool>,
    last_hash: Option<String>,
    last_bucket: Option<String>,
    last_reason: Option<String>,
    held: Option<String>,
    run_id: Option<String>,
    run_tab_id: Option<String>,
    run_bound_at: Option<i64>,
    run_transitions: Option<i64>,
    causal_run_json: Option<String>,
}

fn header(conn: &Connection, id: &str) -> Result<Option<Header>, StorageError> {
    conn.prepare_cached(
        "SELECT s.ordinal, s.creator_tab_id, s.workspace_id, s.team_id, s.cwd, \
         s.agent_authored, s.scenario_id, s.revision, h.version, r.generation, r.next_run, \
         r.expires_at, r.paused, r.stopped, r.last_hash, r.last_bucket, r.last_reason, \
         r.held, r.run_id, r.run_tab_id, r.run_bound_at, r.run_transitions, r.causal_run_json \
         FROM blueprints s LEFT JOIN blueprint_heads h ON h.blueprint_id = s.id \
         LEFT JOIN blueprint_runtime r ON r.blueprint_id = s.id WHERE s.id = ?1",
    )
    .and_then(|mut statement| {
        statement
            .query_row([id], |row| {
                Ok(Header {
                    ordinal: row.get(0)?,
                    creator_tab_id: row.get(1)?,
                    workspace_id: row.get(2)?,
                    team_id: row.get(3)?,
                    cwd: row.get(4)?,
                    agent_authored: row.get(5)?,
                    scenario_id: row.get(6)?,
                    revision: row.get(7)?,
                    version: row.get(8)?,
                    generation: row.get(9)?,
                    next_run: row.get(10)?,
                    expires_at: row.get(11)?,
                    paused: row.get(12)?,
                    stopped: row.get(13)?,
                    last_hash: row.get(14)?,
                    last_bucket: row.get(15)?,
                    last_reason: row.get(16)?,
                    held: row.get(17)?,
                    run_id: row.get(18)?,
                    run_tab_id: row.get(19)?,
                    run_bound_at: row.get(20)?,
                    run_transitions: row.get(21)?,
                    causal_run_json: row.get(22)?,
                })
            })
            .optional()
    })
    .map_err(|e| sqlite_error("read sentinel", e))
}

/// The whole committed aggregate of one stage, or `None` when it does not exist.
pub(super) fn load_one(
    conn: &Connection,
    id: &str,
) -> Result<Option<StoredSentinel>, StorageError> {
    let Some(header) = header(conn, id)? else {
        return Ok(None);
    };
    let missing = || corrupt("stage lacks its head or runtime");
    let version = from_sql_u64("definition version", header.version.ok_or_else(missing)?)?;
    let run = match (header.run_id, header.run_tab_id, header.run_bound_at) {
        (None, None, None) => None,
        (Some(run_id), Some(tab_id), Some(bound_at)) => Some(RunRecord {
            run_id,
            tab_id,
            bound_at: from_sql_u64("run bound time", bound_at)?,
        }),
        _ => return Err(corrupt("partial run binding")),
    };
    let runtime = StageRuntime {
        causal_run_json: header.causal_run_json,
        generation: from_sql_u64("generation", header.generation.ok_or_else(missing)?)?,
        next_run: from_sql_u64("next run", header.next_run.ok_or_else(missing)?)?,
        expires_at: header
            .expires_at
            .map(|at| from_sql_u64("runtime expiry", at))
            .transpose()?,
        paused: header.paused.ok_or_else(missing)?,
        stopped: header.stopped.ok_or_else(missing)?,
        last_hash: header.last_hash,
        last_bucket: header.last_bucket,
        last_reason: header.last_reason,
        held: header.held,
        run,
        run_transitions: u16::try_from(from_sql_u64(
            "run transition count",
            header.run_transitions.ok_or_else(missing)?,
        )?)
        .map_err(|_| corrupt("run transition count exceeds its bound"))?,
    };
    let identity = SentinelIdentity {
        id: SentinelId::trusted(id.to_string()),
        scope: SentinelScope {
            creator_tab_id: header.creator_tab_id,
            workspace_id: header.workspace_id,
            team_id: header.team_id,
        },
        cwd: header.cwd,
        agent_authored: header.agent_authored,
    };
    Ok(Some(StoredSentinel {
        record: SentinelRecord {
            identity,
            scenario_id: header.scenario_id.map(SentinelId::trusted),
            definition: definitions::load_version(conn, id, version)?,
            amendments: definitions::load_lineage(conn, id, version)?,
            runtime,
            transitions: load_transitions(conn, id)?,
        },
        revision: Revision(from_sql_u64("revision", header.revision)?),
        ordinal: from_sql_u64("ordinal", header.ordinal)?,
    }))
}

fn ids(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<String>, StorageError> {
    let mut statement = conn
        .prepare_cached(sql)
        .map_err(|e| sqlite_error("list blueprints", e))?;
    let rows = statement
        .query_map(params, |row| row.get(0))
        .map_err(|e| sqlite_error("list blueprints", e))?;
    let ids: Vec<String> = rows
        .collect::<Result<_, _>>()
        .map_err(|e| sqlite_error("list blueprints", e))?;
    if ids.len() > MAX_SENTINEL_RECORDS {
        return Err(corrupt("inventory exceeds its bound"));
    }
    Ok(ids)
}

fn load_all(conn: &Connection, ids: Vec<String>) -> Result<Vec<StoredSentinel>, StorageError> {
    ids.iter()
        .map(|id| load_one(conn, id)?.ok_or_else(|| corrupt("listed stage vanished")))
        .collect()
}

pub(super) fn inventory(conn: &Connection) -> Result<Vec<StoredSentinel>, StorageError> {
    let ids = ids(conn, "SELECT id FROM blueprints ORDER BY ordinal", [])?;
    load_all(conn, ids)
}

/// One workspace and exact Team scope; a `None` Team never matches a Team-owned stage.
pub(super) fn scope(
    conn: &Connection,
    workspace: &str,
    team: Option<&str>,
) -> Result<Vec<StoredSentinel>, StorageError> {
    let ids = ids(
        conn,
        "SELECT id FROM blueprints WHERE workspace_id = ?1 AND team_id IS ?2 ORDER BY ordinal",
        params![workspace, team],
    )?;
    load_all(conn, ids)
}

pub(super) fn workspace(
    conn: &Connection,
    workspace: &str,
) -> Result<Vec<StoredSentinel>, StorageError> {
    let ids = ids(
        conn,
        "SELECT id FROM blueprints WHERE workspace_id = ?1 ORDER BY ordinal",
        [workspace],
    )?;
    load_all(conn, ids)
}

type ResultRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
);

fn load_transitions(conn: &Connection, id: &str) -> Result<Vec<TransitionRecord>, StorageError> {
    let mut commands = load_commands(conn, id)?;
    let mut results: HashMap<String, Vec<HandlerResultRecord>> = HashMap::new();
    {
        let mut statement = conn
            .prepare_cached(
                "SELECT transition_id, handler_id, status, claim_id, execution_id, accepted_at, \
                 reason FROM blueprint_handler_results WHERE blueprint_id = ?1 \
                 ORDER BY transition_id, ordinal",
            )
            .map_err(|e| sqlite_error("read sentinel results", e))?;
        let rows = statement
            .query_map([id], |row| -> rusqlite::Result<ResultRow> {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .map_err(|e| sqlite_error("read sentinel results", e))?;
        for row in rows {
            let (transition, handler_id, status, claim_id, execution_id, accepted_at, reason) =
                row.map_err(|e| sqlite_error("read sentinel results", e))?;
            let command_result = commands.remove(&(transition.clone(), handler_id.clone()));
            results
                .entry(transition)
                .or_default()
                .push(HandlerResultRecord {
                    handler_id,
                    status: handler_status_from(&status)?,
                    claim_id,
                    execution_id,
                    accepted_at: accepted_at
                        .map(|at| from_sql_u64("claim time", at))
                        .transpose()?,
                    command_result,
                    reason,
                });
        }
    }
    if !commands.is_empty() {
        return Err(corrupt("command result has no handler result"));
    }
    let mut statement = conn
        .prepare_cached(
            "SELECT transition_id, bucket, accepted_at, terminal, definition_version, decision_json, causal_run_json \
             FROM blueprint_transitions WHERE blueprint_id = ?1 ORDER BY ordinal",
        )
        .map_err(|e| sqlite_error("read sentinel transitions", e))?;
    let rows = statement
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(|e| sqlite_error("read sentinel transitions", e))?;
    let mut transitions = Vec::new();
    for row in rows {
        let (transition, bucket, accepted_at, terminal, version, decision_json, causal_run_json) =
            row.map_err(|e| sqlite_error("read sentinel transitions", e))?;
        transitions.push(TransitionRecord {
            causal_run_json,
            decision_json,
            handlers: results.remove(&transition).unwrap_or_default(),
            id: transition,
            bucket,
            accepted_at: from_sql_u64("transition time", accepted_at)?,
            terminal,
            definition_version: version
                .map(|version| from_sql_u64("definition version", version))
                .transpose()?,
        });
    }
    if !results.is_empty() {
        return Err(corrupt("handler result has no transition"));
    }
    Ok(transitions)
}

fn capture(
    total: Option<i64>,
    retained: Option<i64>,
    truncated: Option<bool>,
) -> Result<Option<CaptureRecord>, StorageError> {
    match (total, retained, truncated) {
        (None, None, None) => Ok(None),
        (Some(total), Some(retained), Some(truncated)) => Ok(Some(CaptureRecord {
            total_bytes: from_sql_u64("capture total", total)?,
            retained_bytes: from_sql_u64("capture retained", retained)?,
            truncated,
        })),
        _ => Err(corrupt("partial capture accounting")),
    }
}

type CommandKey = (String, String);

fn load_commands(
    conn: &Connection,
    id: &str,
) -> Result<HashMap<CommandKey, CommandResultRecord>, StorageError> {
    let mut commands = HashMap::new();
    {
        let mut statement = conn
            .prepare_cached(
                "SELECT transition_id, handler_id, status, exit_code, resources_confirmed, \
                 stdout_total, stdout_retained, stdout_truncated, stderr_total, \
                 stderr_retained, stderr_truncated, started_at, finished_at, primary_failure \
                 FROM blueprint_command_results WHERE blueprint_id = ?1",
            )
            .map_err(|e| sqlite_error("read sentinel commands", e))?;
        let mut rows = statement
            .query([id])
            .map_err(|e| sqlite_error("read sentinel commands", e))?;
        while let Some(row) = rows
            .next()
            .map_err(|e| sqlite_error("read sentinel commands", e))?
        {
            let column = |e| sqlite_error("read sentinel commands", e);
            let key: CommandKey = (row.get(0).map_err(column)?, row.get(1).map_err(column)?);
            let status: String = row.get(2).map_err(column)?;
            let record = CommandResultRecord {
                status: command_status_from(&status)?,
                exit_code: row.get(3).map_err(column)?,
                resources_confirmed: row.get(4).map_err(column)?,
                stdout: capture(
                    row.get(5).map_err(column)?,
                    row.get(6).map_err(column)?,
                    row.get(7).map_err(column)?,
                )?,
                stderr: capture(
                    row.get(8).map_err(column)?,
                    row.get(9).map_err(column)?,
                    row.get(10).map_err(column)?,
                )?,
                started_at: from_sql_u64("command started", row.get(11).map_err(column)?)?,
                finished_at: from_sql_u64("command finished", row.get(12).map_err(column)?)?,
                primary: row.get(13).map_err(column)?,
                cleanup: Vec::new(),
            };
            commands.insert(key, record);
        }
    }
    let mut statement = conn
        .prepare_cached(
            "SELECT transition_id, handler_id, error FROM blueprint_command_cleanup \
             WHERE blueprint_id = ?1 ORDER BY transition_id, handler_id, ordinal",
        )
        .map_err(|e| sqlite_error("read sentinel cleanup", e))?;
    let rows = statement
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| sqlite_error("read sentinel cleanup", e))?;
    for row in rows {
        let (transition, handler, error) =
            row.map_err(|e| sqlite_error("read sentinel cleanup", e))?;
        commands
            .get_mut(&(transition, handler))
            .ok_or_else(|| corrupt("cleanup entry has no command result"))?
            .cleanup
            .push(error);
    }
    Ok(commands)
}
