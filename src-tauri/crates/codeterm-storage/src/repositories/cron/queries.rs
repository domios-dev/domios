//! Committed reads of cron identities, definitions, run rows and divergent last runs.

use super::records::{
    CronDivergenceOrigin, CronIdentity, CronJobId, CronJobRecord, CronJobState,
    CronLastRunDivergence, CronRunField, CronRunId, CronRunRecord, CronRunState, StoredCronJob,
};
use crate::error::{sqlite_error, StorageError};
use crate::repositories::{from_sql_u64, Revision};
use rusqlite::{Connection, OptionalExtension, Row};
use std::collections::{BTreeMap, HashMap};

pub(crate) const JOB_COLUMNS: &str = "id, name, description, workspace_id, team_id, schedule, \
     enabled, agent, model, reasoning_effort, task, cwd, role, skill, creator_tab_id, \
     report_target_tab_id, oneshot, max_history, created_at, last_run_id, next_run, \
     jitter_max_ms, tz, timeout_seconds, consecutive_failures, protected";

const RUN_COLUMNS: &str = "job_id, run_id, ordinal, started_at, finished_at, status, tab_id, \
     session_id, cwd, summary, report_delivery";

const MIRROR_COLUMNS: &str = "job_id, run_id, NULL, started_at, finished_at, status, tab_id, \
     session_id, cwd, summary, report_delivery, origin, imported_revision, imported_differing";

fn corrupt(detail: &'static str) -> StorageError {
    StorageError::Corrupt {
        detail: detail.to_string(),
    }
}

fn job_id(text: String) -> Result<CronJobId, StorageError> {
    CronJobId::new(text).map_err(|_| corrupt("malformed cron job id"))
}

fn run_id(text: String) -> Result<CronRunId, StorageError> {
    CronRunId::new(text).map_err(|_| corrupt("malformed cron run id"))
}

fn optional_u64(field: &'static str, value: Option<i64>) -> Result<Option<u64>, StorageError> {
    value.map(|value| from_sql_u64(field, value)).transpose()
}

struct RawJob {
    id: String,
    name: String,
    description: Option<String>,
    workspace_id: String,
    team_id: Option<String>,
    schedule: String,
    enabled: bool,
    agent: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    task: String,
    cwd: Option<String>,
    role: Option<String>,
    skill: Option<String>,
    creator_tab_id: Option<String>,
    report_target_tab_id: Option<String>,
    oneshot: bool,
    max_history: i64,
    created_at: i64,
    last_run_id: Option<String>,
    next_run: Option<i64>,
    jitter_max_ms: Option<i64>,
    tz: Option<String>,
    timeout_seconds: Option<i64>,
    consecutive_failures: i64,
    protected: bool,
}

fn raw_job(row: &Row<'_>) -> rusqlite::Result<RawJob> {
    Ok(RawJob {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        workspace_id: row.get(3)?,
        team_id: row.get(4)?,
        schedule: row.get(5)?,
        enabled: row.get(6)?,
        agent: row.get(7)?,
        model: row.get(8)?,
        reasoning_effort: row.get(9)?,
        task: row.get(10)?,
        cwd: row.get(11)?,
        role: row.get(12)?,
        skill: row.get(13)?,
        creator_tab_id: row.get(14)?,
        report_target_tab_id: row.get(15)?,
        oneshot: row.get(16)?,
        max_history: row.get(17)?,
        created_at: row.get(18)?,
        last_run_id: row.get(19)?,
        next_run: row.get(20)?,
        jitter_max_ms: row.get(21)?,
        tz: row.get(22)?,
        timeout_seconds: row.get(23)?,
        consecutive_failures: row.get(24)?,
        protected: row.get(25)?,
    })
}

fn decode_job(raw: RawJob) -> Result<CronJobRecord, StorageError> {
    Ok(CronJobRecord {
        id: job_id(raw.id)?,
        name: raw.name,
        description: raw.description,
        workspace_id: raw.workspace_id,
        team_id: raw.team_id,
        schedule: raw.schedule,
        enabled: raw.enabled,
        agent: raw.agent,
        model: raw.model,
        reasoning_effort: raw.reasoning_effort,
        task: raw.task,
        cwd: raw.cwd,
        role: raw.role,
        skill: raw.skill,
        creator_tab_id: raw.creator_tab_id,
        report_target_tab_id: raw.report_target_tab_id,
        oneshot: raw.oneshot,
        max_history: from_sql_u64("cron max history", raw.max_history)?,
        created_at: from_sql_u64("cron created at", raw.created_at)?,
        last_run_id: raw.last_run_id.map(run_id).transpose()?,
        next_run: optional_u64("cron next run", raw.next_run)?,
        jitter_max_ms: optional_u64("cron jitter", raw.jitter_max_ms)?,
        tz: raw.tz,
        timeout_seconds: optional_u64("cron timeout", raw.timeout_seconds)?,
        consecutive_failures: u32::try_from(raw.consecutive_failures)
            .map_err(|_| corrupt("cron failure counter out of range"))?,
        protected: raw.protected,
    })
}

struct RawRun {
    job_id: String,
    run_id: String,
    ordinal: Option<i64>,
    started_at: i64,
    finished_at: Option<i64>,
    status: String,
    tab_id: Option<String>,
    session_id: Option<String>,
    cwd: Option<String>,
    summary: Option<String>,
    report_delivery: Option<String>,
}

fn raw_run(row: &Row<'_>) -> rusqlite::Result<RawRun> {
    Ok(RawRun {
        job_id: row.get(0)?,
        run_id: row.get(1)?,
        ordinal: row.get(2)?,
        started_at: row.get(3)?,
        finished_at: row.get(4)?,
        status: row.get(5)?,
        tab_id: row.get(6)?,
        session_id: row.get(7)?,
        cwd: row.get(8)?,
        summary: row.get(9)?,
        report_delivery: row.get(10)?,
    })
}

fn decode_run(raw: RawRun) -> Result<CronRunRecord, StorageError> {
    Ok(CronRunRecord {
        run_id: run_id(raw.run_id)?,
        started_at: from_sql_u64("run start", raw.started_at)?,
        finished_at: optional_u64("run finish", raw.finished_at)?,
        status: CronRunState::from_sql(&raw.status)?,
        tab_id: raw.tab_id,
        session_id: raw.session_id,
        cwd: raw.cwd,
        summary: raw.summary,
        report_delivery: raw.report_delivery,
    })
}

/// Listed history in ordinal order, plus rows kept only as a job's last run.
#[derive(Default)]
struct Runs {
    listed: Vec<CronRunRecord>,
    unlisted: HashMap<CronRunId, CronRunRecord>,
}

fn identities(
    conn: &Connection,
    only: Option<&CronJobId>,
) -> Result<BTreeMap<CronJobId, CronIdentity>, StorageError> {
    let mut statement = conn
        .prepare_cached(
            "SELECT job_id, revision, retired FROM cron_identities \
             WHERE ?1 IS NULL OR job_id = ?1",
        )
        .map_err(|e| sqlite_error("read cron identities", e))?;
    let rows = statement
        .query_map([only.map(CronJobId::as_str)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })
        .map_err(|e| sqlite_error("read cron identities", e))?;
    let mut found = BTreeMap::new();
    for row in rows {
        let (id, revision, retired) = row.map_err(|e| sqlite_error("read cron identities", e))?;
        let identity = CronIdentity {
            revision: Revision(from_sql_u64("revision", revision)?),
            retired,
        };
        found.insert(job_id(id)?, identity);
    }
    Ok(found)
}

fn runs(
    conn: &Connection,
    only: Option<&CronJobId>,
) -> Result<HashMap<CronJobId, Runs>, StorageError> {
    let mut statement = conn
        .prepare_cached(&format!(
            "SELECT {RUN_COLUMNS} FROM cron_runs WHERE ?1 IS NULL OR job_id = ?1 \
             ORDER BY job_id, ordinal"
        ))
        .map_err(|e| sqlite_error("read cron runs", e))?;
    let rows = statement
        .query_map([only.map(CronJobId::as_str)], raw_run)
        .map_err(|e| sqlite_error("read cron runs", e))?;
    let mut grouped: HashMap<CronJobId, Runs> = HashMap::new();
    for row in rows {
        let raw = row.map_err(|e| sqlite_error("read cron runs", e))?;
        let owner = job_id(raw.job_id.clone())?;
        let listed = raw.ordinal.is_some();
        let run = decode_run(raw)?;
        let entry = grouped.entry(owner).or_default();
        if listed {
            entry.listed.push(run);
        } else {
            entry.unlisted.insert(run.run_id.clone(), run);
        }
    }
    Ok(grouped)
}

fn jobs(conn: &Connection, only: Option<&CronJobId>) -> Result<Vec<CronJobRecord>, StorageError> {
    let mut statement = conn
        .prepare_cached(&format!(
            "SELECT {JOB_COLUMNS} FROM cron_jobs WHERE ?1 IS NULL OR id = ?1 ORDER BY ordinal"
        ))
        .map_err(|e| sqlite_error("read cron jobs", e))?;
    let rows = statement
        .query_map([only.map(CronJobId::as_str)], raw_job)
        .map_err(|e| sqlite_error("read cron jobs", e))?;
    rows.map(|row| decode_job(row.map_err(|e| sqlite_error("read cron jobs", e))?))
        .collect()
}

/// Every identity (or only `only`) with its definition and history, in snapshot order.
type Mirror = (CronRunRecord, CronLastRunDivergence);

/// Imported last runs kept apart from the history row with the same run id, by job.
fn mirrors(
    conn: &Connection,
    only: Option<&CronJobId>,
) -> Result<HashMap<CronJobId, Mirror>, StorageError> {
    let mut statement = conn
        .prepare_cached(&format!(
            "SELECT {MIRROR_COLUMNS} FROM cron_last_run_mirrors WHERE ?1 IS NULL OR job_id = ?1"
        ))
        .map_err(|e| sqlite_error("read cron last-run divergence", e))?;
    let rows = statement
        .query_map([only.map(CronJobId::as_str)], |row| {
            Ok((
                raw_run(row)?,
                row.get::<_, String>(11)?,
                row.get::<_, i64>(12)?,
                row.get::<_, String>(13)?,
            ))
        })
        .map_err(|e| sqlite_error("read cron last-run divergence", e))?;
    let mut found = HashMap::new();
    for row in rows {
        let (raw, origin, revision, differing) =
            row.map_err(|e| sqlite_error("read cron last-run divergence", e))?;
        let owner = job_id(raw.job_id.clone())?;
        let origin = match origin.as_str() {
            "legacy_import" => CronDivergenceOrigin::LegacyImport,
            _ => return Err(corrupt("unknown cron divergence origin")),
        };
        let divergence = CronLastRunDivergence {
            origin,
            imported_revision: Revision(from_sql_u64("revision", revision)?),
            imported_differing: CronRunField::parse_list(&differing)?,
        };
        found.insert(owner, (decode_run(raw)?, divergence));
    }
    Ok(found)
}

pub(crate) fn load(
    conn: &Connection,
    only: Option<&CronJobId>,
) -> Result<Vec<CronJobState>, StorageError> {
    let mut identities = identities(conn, only)?;
    let mut runs = runs(conn, only)?;
    let mut mirrors = mirrors(conn, only)?;
    let mut states = Vec::with_capacity(identities.len());
    for record in jobs(conn, only)? {
        let identity = identities
            .remove(&record.id)
            .filter(|identity| !identity.retired)
            .ok_or_else(|| corrupt("cron job without a live identity"))?;
        let mut owned = runs.remove(&record.id).unwrap_or_default();
        let (last_run, divergence) = match (&record.last_run_id, mirrors.remove(&record.id)) {
            (Some(last), Some((copy, divergence))) => {
                let listed = owned.listed.iter().any(|run| run.run_id == *last);
                if copy.run_id != *last || !listed {
                    return Err(corrupt("cron divergent last run lost its history row"));
                }
                (Some(copy), Some(divergence))
            }
            (None, Some(_)) => return Err(corrupt("cron divergent copy without a last run")),
            (None, None) => (None, None),
            (Some(last), None) => {
                let resolved = match owned.listed.iter().find(|run| run.run_id == *last) {
                    Some(listed) => listed.clone(),
                    None => owned
                        .unlisted
                        .remove(last)
                        .ok_or_else(|| corrupt("cron last run row is missing"))?,
                };
                (Some(resolved), None)
            }
        };
        if !owned.unlisted.is_empty() {
            return Err(corrupt("cron run row without history or last-run owner"));
        }
        states.push(CronJobState {
            job_id: record.id.clone(),
            identity,
            job: Some(StoredCronJob {
                record,
                last_run,
                divergence,
            }),
            history: owned.listed,
        });
    }
    for (id, identity) in identities {
        if !identity.retired {
            return Err(corrupt("live cron identity without a definition"));
        }
        let owned = runs.remove(&id).unwrap_or_default();
        if !owned.unlisted.is_empty() {
            return Err(corrupt("retired cron identity keeps a last-run row"));
        }
        states.push(CronJobState {
            job_id: id,
            identity,
            job: None,
            history: owned.listed,
        });
    }
    if !runs.is_empty() {
        return Err(corrupt("cron runs without an identity"));
    }
    if !mirrors.is_empty() {
        return Err(corrupt("cron divergent copy without a live definition"));
    }
    Ok(states)
}

/// The run id of a job's divergent last-run copy, if one is stored.
pub(crate) fn mirror_run_of(
    conn: &Connection,
    id: &CronJobId,
) -> Result<Option<String>, StorageError> {
    conn.prepare_cached("SELECT run_id FROM cron_last_run_mirrors WHERE job_id = ?1")
        .and_then(|mut statement| {
            statement
                .query_row([id.as_str()], |row| row.get(0))
                .optional()
        })
        .map_err(|e| sqlite_error("read cron last-run divergence", e))
}

pub(crate) fn identity(
    conn: &Connection,
    id: &CronJobId,
) -> Result<Option<CronIdentity>, StorageError> {
    let stored: Option<(i64, bool)> = conn
        .prepare_cached("SELECT revision, retired FROM cron_identities WHERE job_id = ?1")
        .and_then(|mut statement| {
            statement
                .query_row([id.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
                .optional()
        })
        .map_err(|e| sqlite_error("read cron identity", e))?;
    stored
        .map(|(revision, retired)| {
            Ok(CronIdentity {
                revision: Revision(from_sql_u64("revision", revision)?),
                retired,
            })
        })
        .transpose()
}

/// `None` when no live definition exists; otherwise its current last-run reference.
pub(crate) fn last_run_of(
    conn: &Connection,
    id: &CronJobId,
) -> Result<Option<Option<String>>, StorageError> {
    conn.prepare_cached("SELECT last_run_id FROM cron_jobs WHERE id = ?1")
        .and_then(|mut statement| {
            statement
                .query_row([id.as_str()], |row| row.get(0))
                .optional()
        })
        .map_err(|e| sqlite_error("read cron last run", e))
}
