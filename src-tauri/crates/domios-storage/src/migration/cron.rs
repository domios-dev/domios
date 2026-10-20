//! Cron join: the accepted repository bounds, `write_import`, and an independent SQL
//! projection of the written rows compared with the decoded original before commit.

use super::compare::{self, Diff, ObservedDivergence};
use super::manifest::{bounded, token, ExpectedDivergence, Facts, ImportRefusal};
use super::source_inventory::Sha256Digest;
use super::{Halt, ImportDomain};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::cron::{
    write_import, CronDivergenceOrigin, CronHistoryImport, CronImport, CronJobId, CronJobImport,
    CronJobRecord, CronLastRunDivergence, CronRunField, CronRunId, CronRunRecord, CronRunState,
    MAX_CRON_BATCH, MAX_CRON_HISTORY, MAX_CRON_JOBS,
};
use crate::repositories::{from_sql_u64, Revision};
use rusqlite::types::FromSql;
use rusqlite::{Connection, Row};
use std::collections::{BTreeMap, HashSet};

const DOMAIN: ImportDomain = ImportDomain::Cron;

const JOBS: &str = "SELECT id, name, description, workspace_id, team_id, schedule, enabled, \
     agent, model, reasoning_effort, task, cwd, role, skill, creator_tab_id, \
     report_target_tab_id, oneshot, max_history, created_at, last_run_id, next_run, \
     jitter_max_ms, tz, timeout_seconds, consecutive_failures, protected \
     FROM cron_jobs ORDER BY ordinal";
const RUNS: &str = "SELECT run_id, started_at, finished_at, status, tab_id, session_id, cwd, \
     summary, report_delivery, job_id, ordinal FROM cron_runs ORDER BY job_id, ordinal";
const MIRRORS: &str = "SELECT run_id, started_at, finished_at, status, tab_id, session_id, \
     cwd, summary, report_delivery, job_id, origin, imported_revision, imported_differing \
     FROM cron_last_run_mirrors ORDER BY job_id";
const IDENTITIES: &str = "SELECT job_id, retired FROM cron_identities ORDER BY job_id";

fn bound(name: &'static str, limit: u64, observed: u64) -> Result<(), ImportRefusal> {
    if observed > limit {
        return Err(ImportRefusal::Bound {
            domain: DOMAIN,
            bound: name,
            limit,
            observed,
        });
    }
    Ok(())
}

/// Any exceeded repository bound refuses the whole import; nothing is trimmed or clamped.
pub(super) fn preflight(import: &CronImport, facts: &mut Facts) -> Result<(), ImportRefusal> {
    let jobs = import.jobs.len() as u64;
    let longest = import
        .histories
        .iter()
        .map(|history| history.runs.len() as u64)
        .max()
        .unwrap_or(0);
    let widest = import
        .jobs
        .iter()
        .map(|job| job.record.max_history)
        .max()
        .unwrap_or(0);
    bound("jobs", MAX_CRON_JOBS as u64, jobs)?;
    bound("history-per-job", MAX_CRON_HISTORY, longest)?;
    bound("max-history", MAX_CRON_HISTORY, widest)?;
    import.validate().map_err(|error| ImportRefusal::Invalid {
        domain: DOMAIN,
        detail: bounded(&error.to_string()),
    })?;
    facts.bound(DOMAIN, "jobs", MAX_CRON_JOBS as u64, jobs);
    facts.bound(DOMAIN, "history-per-job", MAX_CRON_HISTORY, longest);
    facts.bound(DOMAIN, "max-history", MAX_CRON_HISTORY, widest);
    facts.bound(DOMAIN, "write-batch", MAX_CRON_BATCH as u64, 0);
    facts.rule(DOMAIN, "write-batch", "typed-cronwrite-batches-only");
    facts.rule(DOMAIN, "fields", "repository-validated");
    let live: HashSet<&CronJobId> = import.jobs.iter().map(|job| &job.record.id).collect();
    let listed = |job: &CronJobImport, run: &CronRunRecord| {
        import.histories.iter().any(|history| {
            history.job_id == job.record.id
                && history.runs.iter().any(|row| row.run_id == run.run_id)
        })
    };
    let unlisted = import
        .jobs
        .iter()
        .filter(|job| job.last_run.as_ref().is_some_and(|run| !listed(job, run)))
        .count();
    let retired = import
        .histories
        .iter()
        .filter(|history| !live.contains(&history.job_id))
        .count();
    let empty_live = import
        .histories
        .iter()
        .filter(|history| history.runs.is_empty() && live.contains(&history.job_id))
        .count();
    let runs: u64 = import
        .histories
        .iter()
        .map(|history| history.runs.len() as u64)
        .sum();
    let divergences = expected_divergences(import);
    facts.count(DOMAIN, "jobs", jobs);
    facts.count(DOMAIN, "history-lists", import.histories.len() as u64);
    facts.count(DOMAIN, "history-runs", runs);
    facts.count(DOMAIN, "retired-identities", retired as u64);
    facts.count(DOMAIN, "empty-live-histories", empty_live as u64);
    facts.count(DOMAIN, "unlisted-last-runs", unlisted as u64);
    facts.count(DOMAIN, "divergent-last-runs", divergences.len() as u64);
    facts.divergences.extend(divergences);
    Ok(())
}

/// Every job whose own last run differs from its history row with the same run id.
pub(super) fn expected_divergences(import: &CronImport) -> Vec<ExpectedDivergence> {
    import
        .jobs
        .iter()
        .filter_map(|job| {
            let copy = job.last_run.as_ref()?;
            let listed = import.divergent_row(job)?;
            Some(ExpectedDivergence {
                job_id: job.record.id.clone(),
                run_id: copy.run_id.clone(),
                origin: CronDivergenceOrigin::LegacyImport,
                imported_differing: CronRunField::differing(copy, listed),
                history_copy_sha256: run_digest(listed),
                last_run_copy_sha256: run_digest(copy),
            })
        })
        .collect()
}

/// Content digest of one run copy over a canonical, unambiguous line encoding.
fn run_digest(run: &CronRunRecord) -> Sha256Digest {
    let fields = [
        Some(run.run_id.as_str().to_string()),
        Some(run.started_at.to_string()),
        run.finished_at.map(|at| at.to_string()),
        Some(run.status.as_sql().to_string()),
        run.tab_id.clone(),
        run.session_id.clone(),
        run.cwd.clone(),
        run.summary.clone(),
        run.report_delivery.clone(),
    ];
    let mut text = String::new();
    for field in fields {
        match field {
            Some(value) => {
                text.push('+');
                text.push_str(&token(&value));
            }
            None => text.push('-'),
        }
        text.push('\n');
    }
    Sha256Digest::of(text.as_bytes())
}

pub(super) fn occupied(conn: &Connection) -> Result<bool, StorageError> {
    conn.query_row("SELECT EXISTS (SELECT 1 FROM cron_identities)", [], |row| {
        row.get(0)
    })
    .map_err(|e| sqlite_error("inspect cron domain", e))
}

pub(super) fn write(cx: &mut WriteCx<'_>, import: &CronImport) -> Result<(), StorageError> {
    write_import(cx, import)
}

pub(super) fn verify(
    conn: &Connection,
    import: &CronImport,
    divergences: &[ExpectedDivergence],
    revision: Revision,
) -> Result<(), Halt> {
    let projection = project(conn)?;
    let mut diff = Diff::new(DOMAIN);
    compare::diff_cron(
        &compare::normalized_cron(import),
        &projection.import,
        &mut diff,
    );
    compare::diff_divergences(divergences, &projection.divergences, revision, &mut diff);
    for (subject, field) in &projection.anomalies {
        diff.note(subject, field);
    }
    diff.gate()?;
    Ok(())
}

struct Projection {
    import: CronImport,
    divergences: Vec<ObservedDivergence>,
    anomalies: Vec<(String, &'static str)>,
}

fn corrupt(detail: &str) -> StorageError {
    StorageError::Corrupt {
        detail: format!("cron projection: {detail}"),
    }
}

fn col<T: FromSql>(row: &Row<'_>, index: usize) -> Result<T, StorageError> {
    row.get(index)
        .map_err(|e| sqlite_error("read cron projection", e))
}

fn unsigned(row: &Row<'_>, index: usize, field: &'static str) -> Result<u64, StorageError> {
    from_sql_u64(field, col(row, index)?)
}

fn optional_unsigned(
    row: &Row<'_>,
    index: usize,
    field: &'static str,
) -> Result<Option<u64>, StorageError> {
    col::<Option<i64>>(row, index)?
        .map(|value| from_sql_u64(field, value))
        .transpose()
}

fn job_id(text: String) -> Result<CronJobId, StorageError> {
    CronJobId::new(text).map_err(|_| corrupt("malformed job id"))
}

fn run_id(text: String) -> Result<CronRunId, StorageError> {
    CronRunId::new(text).map_err(|_| corrupt("malformed run id"))
}

fn each(
    conn: &Connection,
    sql: &str,
    mut visit: impl FnMut(&Row<'_>) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|e| sqlite_error("read cron projection", e))?;
    let mut rows = statement
        .query([])
        .map_err(|e| sqlite_error("read cron projection", e))?;
    while let Some(row) = rows
        .next()
        .map_err(|e| sqlite_error("read cron projection", e))?
    {
        visit(row)?;
    }
    Ok(())
}

/// The nine run columns every run query selects first.
fn run_at(row: &Row<'_>) -> Result<CronRunRecord, StorageError> {
    Ok(CronRunRecord {
        run_id: run_id(col(row, 0)?)?,
        started_at: unsigned(row, 1, "run start")?,
        finished_at: optional_unsigned(row, 2, "run finish")?,
        status: CronRunState::from_sql(&col::<String>(row, 3)?)?,
        tab_id: col(row, 4)?,
        session_id: col(row, 5)?,
        cwd: col(row, 6)?,
        summary: col(row, 7)?,
        report_delivery: col(row, 8)?,
    })
}

fn job_record(row: &Row<'_>) -> Result<CronJobRecord, StorageError> {
    Ok(CronJobRecord {
        id: job_id(col(row, 0)?)?,
        name: col(row, 1)?,
        description: col(row, 2)?,
        workspace_id: col(row, 3)?,
        team_id: col(row, 4)?,
        schedule: col(row, 5)?,
        enabled: col(row, 6)?,
        agent: col(row, 7)?,
        model: col(row, 8)?,
        reasoning_effort: col(row, 9)?,
        task: col(row, 10)?,
        cwd: col(row, 11)?,
        role: col(row, 12)?,
        skill: col(row, 13)?,
        creator_tab_id: col(row, 14)?,
        report_target_tab_id: col(row, 15)?,
        oneshot: col(row, 16)?,
        max_history: unsigned(row, 17, "cron max history")?,
        created_at: unsigned(row, 18, "cron created at")?,
        last_run_id: col::<Option<String>>(row, 19)?.map(run_id).transpose()?,
        next_run: optional_unsigned(row, 20, "cron next run")?,
        jitter_max_ms: optional_unsigned(row, 21, "cron jitter")?,
        tz: col(row, 22)?,
        timeout_seconds: optional_unsigned(row, 23, "cron timeout")?,
        consecutive_failures: u32::try_from(col::<i64>(row, 24)?)
            .map_err(|_| corrupt("consecutive failures out of range"))?,
        protected: col(row, 25)?,
    })
}

fn divergence_at(row: &Row<'_>) -> Result<CronLastRunDivergence, StorageError> {
    let origin = match col::<String>(row, 10)?.as_str() {
        "legacy_import" => CronDivergenceOrigin::LegacyImport,
        _ => return Err(corrupt("unknown divergence origin")),
    };
    Ok(CronLastRunDivergence {
        origin,
        imported_revision: Revision(unsigned(row, 11, "imported revision")?),
        imported_differing: CronRunField::parse_list(&col::<String>(row, 12)?)?,
    })
}

/// Rebuilds the import shape from rows without the repository's reader, so a mapping
/// defect on either side shows up as a difference.
fn project(conn: &Connection) -> Result<Projection, StorageError> {
    let mut runs: BTreeMap<String, Vec<(bool, CronRunRecord)>> = BTreeMap::new();
    each(conn, RUNS, |row| {
        let run = run_at(row)?;
        let listed = col::<Option<i64>>(row, 10)?.is_some();
        runs.entry(col(row, 9)?).or_default().push((listed, run));
        Ok(())
    })?;
    let mut mirrors: BTreeMap<String, (CronRunRecord, CronLastRunDivergence)> = BTreeMap::new();
    each(conn, MIRRORS, |row| {
        mirrors.insert(col(row, 9)?, (run_at(row)?, divergence_at(row)?));
        Ok(())
    })?;
    let mut identities: Vec<(String, bool)> = Vec::new();
    each(conn, IDENTITIES, |row| {
        identities.push((col(row, 0)?, col(row, 1)?));
        Ok(())
    })?;
    let mut jobs = Vec::new();
    let mut anomalies = Vec::new();
    let mut referenced: HashSet<(String, String)> = HashSet::new();
    each(conn, JOBS, |row| {
        let record = job_record(row)?;
        let last_run = match &record.last_run_id {
            None => None,
            Some(last) => {
                let owner = record.id.as_str().to_string();
                referenced.insert((owner.clone(), last.as_str().to_string()));
                let stored = runs
                    .get(&owner)
                    .and_then(|list| list.iter().find(|(_, run)| run.run_id == *last))
                    .map(|(_, run)| run.clone());
                if stored.is_none() {
                    anomalies.push((owner.clone(), "last_run_row"));
                }
                mirrors.get(&owner).map(|(copy, _)| copy.clone()).or(stored)
            }
        };
        jobs.push(CronJobImport { record, last_run });
        Ok(())
    })?;
    let live: HashSet<String> = jobs
        .iter()
        .map(|job: &CronJobImport| job.record.id.as_str().to_string())
        .collect();
    let mut histories = Vec::new();
    for (id, retired) in identities {
        if retired == live.contains(&id) {
            anomalies.push((id.clone(), "retired_flag"));
        }
        let listed: Vec<CronRunRecord> = runs
            .get(&id)
            .map(|list| {
                list.iter()
                    .filter(|(listed, _)| *listed)
                    .map(|(_, run)| run.clone())
                    .collect()
            })
            .unwrap_or_default();
        if !listed.is_empty() || retired {
            histories.push(CronHistoryImport {
                job_id: job_id(id)?,
                runs: listed,
            });
        }
    }
    for (owner, list) in &runs {
        for (listed, run) in list {
            let key = (owner.clone(), run.run_id.as_str().to_string());
            if !listed && !referenced.contains(&key) {
                anomalies.push((owner.clone(), "unreferenced_last_run_row"));
            }
        }
    }
    let mut divergences = Vec::with_capacity(mirrors.len());
    for (owner, (copy, divergence)) in mirrors {
        divergences.push(ObservedDivergence {
            job_id: job_id(owner)?,
            run_id: copy.run_id,
            divergence,
        });
    }
    Ok(Projection {
        import: CronImport { jobs, histories },
        divergences,
        anomalies,
    })
}
