//! Cron writes inside the writer's transaction; each identity-scoped write checks its revision.

use super::queries::{self, JOB_COLUMNS};
use super::records::{
    CronApplied, CronHistoryDisposition, CronImport, CronJobId, CronJobRecord, CronRunField,
    CronRunId, CronRunRecord, CronScope, CronWrite, MAX_CRON_JOBS,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{expect_matches, to_sql_u64, Expect, Revision};
use rusqlite::{params, types::ToSql};
use std::collections::HashSet;

fn entity(id: &CronJobId) -> String {
    format!("cron job {}", id.as_str())
}

fn sql_revision(cx: &WriteCx<'_>) -> Result<i64, StorageError> {
    to_sql_u64("revision", cx.revision.0)
}

fn optional_sql(field: &'static str, value: Option<u64>) -> Result<Option<i64>, StorageError> {
    value.map(|value| to_sql_u64(field, value)).transpose()
}

pub(crate) fn apply(
    cx: &mut WriteCx<'_>,
    batch: Vec<CronWrite>,
) -> Result<CronApplied, StorageError> {
    let mut applied = CronApplied::default();
    for write in batch {
        match write {
            CronWrite::Create(job) => create(cx, &job)?,
            CronWrite::Patch { job, expect } => patch(cx, &job, expect)?,
            CronWrite::StartRun { job, run, expect } => start_run(cx, &job, &run, expect)?,
            CronWrite::UpdateRuns {
                job_id,
                expect,
                runs,
                job,
                last_run,
            } => update_runs(cx, &job_id, expect, &runs, job.as_ref(), last_run.as_ref())?,
            CronWrite::Remove {
                id,
                expect,
                history,
            } => {
                remove(cx, &id, expect, history)?;
                applied.removed.push(id);
            }
            CronWrite::RemoveScope(scope) => applied.removed.extend(remove_scope(cx, &scope)?),
        }
    }
    Ok(applied)
}

/// A retired identity has history but no definition, so it refuses definition writes.
fn expect_identity(
    cx: &WriteCx<'_>,
    id: &CronJobId,
    expect: Revision,
    live: bool,
) -> Result<(), StorageError> {
    let current = queries::identity(cx.conn, id)?;
    expect_matches(
        &entity(id),
        Expect::At(expect),
        current.map(|identity| identity.revision),
    )?;
    if live && current.is_some_and(|identity| identity.retired) {
        return Err(StorageError::NotFound { entity: entity(id) });
    }
    Ok(())
}

fn touch(cx: &mut WriteCx<'_>, id: &CronJobId) -> Result<(), StorageError> {
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE cron_identities SET revision = ?2 WHERE job_id = ?1",
        params![id.0, revision],
    )?;
    Ok(())
}

/// Definition values after `id`, in `JOB_COLUMNS` order.
struct JobValues<'a> {
    job: &'a CronJobRecord,
    last_run_id: Option<&'a str>,
    max_history: i64,
    created_at: i64,
    next_run: Option<i64>,
    jitter_max_ms: Option<i64>,
    timeout_seconds: Option<i64>,
    consecutive_failures: i64,
}

impl<'a> JobValues<'a> {
    fn of(job: &'a CronJobRecord) -> Result<Self, StorageError> {
        Ok(JobValues {
            job,
            last_run_id: job.last_run_id.as_ref().map(CronRunId::as_str),
            max_history: to_sql_u64("cron max history", job.max_history)?,
            created_at: to_sql_u64("cron created at", job.created_at)?,
            next_run: optional_sql("cron next run", job.next_run)?,
            jitter_max_ms: optional_sql("cron jitter", job.jitter_max_ms)?,
            timeout_seconds: optional_sql("cron timeout", job.timeout_seconds)?,
            consecutive_failures: i64::from(job.consecutive_failures),
        })
    }

    fn scalars(&self) -> [&dyn ToSql; 25] {
        let job = self.job;
        [
            &job.name,
            &job.description,
            &job.workspace_id,
            &job.team_id,
            &job.schedule,
            &job.enabled,
            &job.agent,
            &job.model,
            &job.reasoning_effort,
            &job.task,
            &job.cwd,
            &job.role,
            &job.skill,
            &job.creator_tab_id,
            &job.report_target_tab_id,
            &job.oneshot,
            &self.max_history,
            &self.created_at,
            &self.last_run_id,
            &self.next_run,
            &self.jitter_max_ms,
            &job.tz,
            &self.timeout_seconds,
            &self.consecutive_failures,
            &job.protected,
        ]
    }
}

fn insert_job_row(
    cx: &mut WriteCx<'_>,
    job: &CronJobRecord,
    ordinal: Option<i64>,
) -> Result<(), StorageError> {
    let values = JobValues::of(job)?;
    let mut bound: Vec<&dyn ToSql> = vec![&job.id.0 as &dyn ToSql, &ordinal];
    bound.extend(values.scalars());
    let placeholders: Vec<String> = (3..=27).map(|index| format!("?{index}")).collect();
    let columns = JOB_COLUMNS.replacen("id, ", "id, ordinal, ", 1);
    cx.write(
        &format!(
            "INSERT INTO cron_jobs ({columns}) VALUES (?1, \
             COALESCE(?2, (SELECT COALESCE(MAX(ordinal), -1) + 1 FROM cron_jobs)), {})",
            placeholders.join(", ")
        ),
        bound.as_slice(),
    )?;
    Ok(())
}

fn update_job_row(cx: &mut WriteCx<'_>, job: &CronJobRecord) -> Result<(), StorageError> {
    let values = JobValues::of(job)?;
    let mut bound: Vec<&dyn ToSql> = vec![&job.id.0 as &dyn ToSql];
    bound.extend(values.scalars());
    let assignments: Vec<String> = JOB_COLUMNS
        .split(", ")
        .skip(1)
        .zip(2..)
        .map(|(column, index)| format!("{} = ?{index}", column.trim()))
        .collect();
    let changed = cx.write(
        &format!(
            "UPDATE cron_jobs SET {} WHERE id = ?1",
            assignments.join(", ")
        ),
        bound.as_slice(),
    )?;
    if changed != 1 {
        return Err(StorageError::NotFound {
            entity: entity(&job.id),
        });
    }
    Ok(())
}

fn run_values(run: &CronRunRecord) -> Result<(i64, Option<i64>), StorageError> {
    Ok((
        to_sql_u64("run start", run.started_at)?,
        optional_sql("run finish", run.finished_at)?,
    ))
}

fn insert_run_row(
    cx: &mut WriteCx<'_>,
    job_id: &CronJobId,
    run: &CronRunRecord,
    ordinal: Option<i64>,
) -> Result<(), StorageError> {
    let (started_at, finished_at) = run_values(run)?;
    cx.write(
        "INSERT INTO cron_runs (job_id, run_id, ordinal, started_at, finished_at, status, \
         tab_id, session_id, cwd, summary, report_delivery) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            job_id.0,
            run.run_id.0,
            ordinal,
            started_at,
            finished_at,
            run.status.as_sql(),
            run.tab_id,
            run.session_id,
            run.cwd,
            run.summary,
            run.report_delivery
        ],
    )?;
    Ok(())
}

fn update_run_row(
    cx: &mut WriteCx<'_>,
    job_id: &CronJobId,
    run: &CronRunRecord,
) -> Result<(), StorageError> {
    let (started_at, finished_at) = run_values(run)?;
    let changed = cx.write(
        "UPDATE cron_runs SET started_at = ?3, finished_at = ?4, status = ?5, tab_id = ?6, \
         session_id = ?7, cwd = ?8, summary = ?9, report_delivery = ?10 \
         WHERE job_id = ?1 AND run_id = ?2",
        params![
            job_id.0,
            run.run_id.0,
            started_at,
            finished_at,
            run.status.as_sql(),
            run.tab_id,
            run.session_id,
            run.cwd,
            run.summary,
            run.report_delivery
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::NotFound {
            entity: format!("{} run {}", entity(job_id), run.run_id.0),
        });
    }
    Ok(())
}

fn create(cx: &mut WriteCx<'_>, job: &CronJobRecord) -> Result<(), StorageError> {
    let current = queries::identity(cx.conn, &job.id)?;
    expect_matches(
        &entity(&job.id),
        Expect::Absent,
        current.map(|identity| identity.revision),
    )?;
    if job.last_run_id.is_some() {
        return Err(StorageError::invalid(
            "cron job",
            "a new definition has no last run",
        ));
    }
    let live: i64 = cx
        .conn
        .query_row("SELECT COUNT(*) FROM cron_jobs", [], |row| row.get(0))
        .map_err(|e| sqlite_error("count cron jobs", e))?;
    if live >= MAX_CRON_JOBS as i64 {
        return Err(StorageError::invalid(
            "cron jobs",
            "inventory is at its bound",
        ));
    }
    let revision = sql_revision(cx)?;
    cx.write(
        "INSERT INTO cron_identities (job_id, revision, retired) VALUES (?1, ?2, 0)",
        params![job.id.0, revision],
    )?;
    insert_job_row(cx, job, None)
}

/// A definition write never moves the last run; only run starts do.
fn require_last_run(
    cx: &WriteCx<'_>,
    job: &CronJobRecord,
    operation: &'static str,
) -> Result<(), StorageError> {
    let current =
        queries::last_run_of(cx.conn, &job.id)?.ok_or_else(|| StorageError::NotFound {
            entity: entity(&job.id),
        })?;
    if current.as_deref() != job.last_run_id.as_ref().map(CronRunId::as_str) {
        return Err(StorageError::invalid(
            operation,
            "cannot move the job's last run",
        ));
    }
    Ok(())
}

fn patch(cx: &mut WriteCx<'_>, job: &CronJobRecord, expect: Revision) -> Result<(), StorageError> {
    expect_identity(cx, &job.id, expect, true)?;
    require_last_run(cx, job, "cron patch")?;
    update_job_row(cx, job)?;
    touch(cx, &job.id)
}

fn next_ordinal(cx: &WriteCx<'_>, job_id: &CronJobId) -> Result<i64, StorageError> {
    cx.conn
        .query_row(
            "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM cron_runs WHERE job_id = ?1",
            [job_id.as_str()],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_error("read cron history order", e))
}

fn start_run(
    cx: &mut WriteCx<'_>,
    job: &CronJobRecord,
    run: &CronRunRecord,
    expect: Revision,
) -> Result<(), StorageError> {
    expect_identity(cx, &job.id, expect, true)?;
    let previous =
        queries::last_run_of(cx.conn, &job.id)?.ok_or_else(|| StorageError::NotFound {
            entity: entity(&job.id),
        })?;
    let ordinal = next_ordinal(cx, &job.id)?;
    insert_run_row(cx, &job.id, run, Some(ordinal))?;
    drop_mirror(cx, &job.id)?;
    update_job_row(cx, job)?;
    trim_history(cx, &job.id, job.max_history, &run.run_id)?;
    if let Some(previous) = previous.filter(|previous| *previous != run.run_id.0) {
        // A row kept only as the previous last run has no owner left.
        cx.write(
            "DELETE FROM cron_runs WHERE job_id = ?1 AND run_id = ?2 AND ordinal IS NULL",
            params![job.id.0, previous],
        )?;
    }
    touch(cx, &job.id)
}

/// Drops the oldest listed runs beyond `max`; the job's last run stays as an unlisted row.
fn trim_history(
    cx: &mut WriteCx<'_>,
    job_id: &CronJobId,
    max: u64,
    last_run: &CronRunId,
) -> Result<(), StorageError> {
    let listed: i64 = cx
        .conn
        .query_row(
            "SELECT COUNT(*) FROM cron_runs WHERE job_id = ?1 AND ordinal IS NOT NULL",
            [job_id.as_str()],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_error("count cron history", e))?;
    let excess = listed - to_sql_u64("cron max history", max)?;
    if excess <= 0 {
        return Ok(());
    }
    let oldest: Vec<String> = {
        let mut statement = cx
            .conn
            .prepare_cached(
                "SELECT run_id FROM cron_runs WHERE job_id = ?1 AND ordinal IS NOT NULL \
                 ORDER BY ordinal LIMIT ?2",
            )
            .map_err(|e| sqlite_error("read cron history order", e))?;
        let rows = statement
            .query_map(params![job_id.0, excess], |row| row.get(0))
            .map_err(|e| sqlite_error("read cron history order", e))?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(|e| sqlite_error("read cron history order", e))?
    };
    for run_id in oldest {
        let sql = if run_id == last_run.0 {
            "UPDATE cron_runs SET ordinal = NULL WHERE job_id = ?1 AND run_id = ?2"
        } else {
            "DELETE FROM cron_runs WHERE job_id = ?1 AND run_id = ?2"
        };
        cx.write(sql, params![job_id.0, run_id])?;
    }
    Ok(())
}

fn update_runs(
    cx: &mut WriteCx<'_>,
    job_id: &CronJobId,
    expect: Revision,
    runs: &[CronRunRecord],
    job: Option<&CronJobRecord>,
    last_run: Option<&CronRunRecord>,
) -> Result<(), StorageError> {
    expect_identity(cx, job_id, expect, job.is_some() || last_run.is_some())?;
    for run in runs {
        update_run_row(cx, job_id, run)?;
    }
    if let Some(job) = job {
        require_last_run(cx, job, "cron run update")?;
        update_job_row(cx, job)?;
    }
    if let Some(last_run) = last_run {
        evolve_mirror(cx, job_id, last_run)?;
    }
    if !runs.is_empty() || last_run.is_some() {
        drop_converged_mirror(cx, job_id)?;
    }
    touch(cx, job_id)
}

/// Only an imported divergence can be evolved; no write ever creates a new one.
fn evolve_mirror(
    cx: &mut WriteCx<'_>,
    job_id: &CronJobId,
    copy: &CronRunRecord,
) -> Result<(), StorageError> {
    match queries::mirror_run_of(cx.conn, job_id)? {
        None => Err(StorageError::invalid(
            "cron last run",
            "a divergent last-run copy is created only by import",
        )),
        Some(run_id) if run_id != copy.run_id.0 => Err(StorageError::invalid(
            "cron last run",
            "the divergent copy belongs to another run",
        )),
        Some(_) => {
            let (started_at, finished_at) = run_values(copy)?;
            cx.write(
                "UPDATE cron_last_run_mirrors SET started_at = ?2, finished_at = ?3, \
                 status = ?4, tab_id = ?5, session_id = ?6, cwd = ?7, summary = ?8, \
                 report_delivery = ?9 WHERE job_id = ?1",
                params![
                    job_id.0,
                    started_at,
                    finished_at,
                    copy.status.as_sql(),
                    copy.tab_id,
                    copy.session_id,
                    copy.cwd,
                    copy.summary,
                    copy.report_delivery
                ],
            )?;
            Ok(())
        }
    }
}

/// A copy equal to its history row again is no longer a divergence.
fn drop_converged_mirror(cx: &mut WriteCx<'_>, job_id: &CronJobId) -> Result<(), StorageError> {
    cx.write(
        "DELETE FROM cron_last_run_mirrors WHERE job_id = ?1 AND EXISTS (\
         SELECT 1 FROM cron_runs r WHERE r.job_id = cron_last_run_mirrors.job_id \
         AND r.run_id = cron_last_run_mirrors.run_id \
         AND r.started_at = cron_last_run_mirrors.started_at \
         AND r.finished_at IS cron_last_run_mirrors.finished_at \
         AND r.status = cron_last_run_mirrors.status \
         AND r.tab_id IS cron_last_run_mirrors.tab_id \
         AND r.session_id IS cron_last_run_mirrors.session_id \
         AND r.cwd IS cron_last_run_mirrors.cwd \
         AND r.summary IS cron_last_run_mirrors.summary \
         AND r.report_delivery IS cron_last_run_mirrors.report_delivery)",
        [job_id.as_str()],
    )?;
    Ok(())
}

/// The current divergent copy goes with its definition or its last run; originals
/// stay in the importer's backup.
fn drop_mirror(cx: &mut WriteCx<'_>, job_id: &CronJobId) -> Result<(), StorageError> {
    cx.write(
        "DELETE FROM cron_last_run_mirrors WHERE job_id = ?1",
        [job_id.as_str()],
    )?;
    Ok(())
}

fn drop_identity(cx: &mut WriteCx<'_>, id: &CronJobId) -> Result<(), StorageError> {
    cx.write("DELETE FROM cron_runs WHERE job_id = ?1", [id.as_str()])?;
    cx.write(
        "DELETE FROM cron_identities WHERE job_id = ?1",
        [id.as_str()],
    )?;
    Ok(())
}

fn remove(
    cx: &mut WriteCx<'_>,
    id: &CronJobId,
    expect: Revision,
    history: CronHistoryDisposition,
) -> Result<(), StorageError> {
    expect_identity(cx, id, expect, true)?;
    drop_mirror(cx, id)?;
    if cx.write("DELETE FROM cron_jobs WHERE id = ?1", [id.as_str()])? != 1 {
        return Err(StorageError::NotFound { entity: entity(id) });
    }
    match history {
        CronHistoryDisposition::Drop => drop_identity(cx, id),
        CronHistoryDisposition::Keep => {
            // The last-run-only row belonged to the definition, not to the history.
            cx.write(
                "DELETE FROM cron_runs WHERE job_id = ?1 AND ordinal IS NULL",
                [id.as_str()],
            )?;
            let revision = sql_revision(cx)?;
            cx.write(
                "UPDATE cron_identities SET retired = 1, revision = ?2 WHERE job_id = ?1",
                params![id.0, revision],
            )?;
            Ok(())
        }
    }
}

fn remove_scope(cx: &mut WriteCx<'_>, scope: &CronScope) -> Result<Vec<CronJobId>, StorageError> {
    let (sql, value) = match scope {
        CronScope::Workspace(workspace) => (
            "SELECT id FROM cron_jobs WHERE workspace_id = ?1 ORDER BY ordinal",
            workspace,
        ),
        CronScope::Team(team) => (
            "SELECT id FROM cron_jobs WHERE team_id = ?1 ORDER BY ordinal",
            team,
        ),
    };
    let ids: Vec<String> = {
        let mut statement = cx
            .conn
            .prepare_cached(sql)
            .map_err(|e| sqlite_error("read cron scope", e))?;
        let rows = statement
            .query_map([value.as_str()], |row| row.get(0))
            .map_err(|e| sqlite_error("read cron scope", e))?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(|e| sqlite_error("read cron scope", e))?
    };
    let mut removed = Vec::with_capacity(ids.len());
    for id in ids {
        let id = CronJobId::new(id).map_err(|_| StorageError::Corrupt {
            detail: "malformed cron job id".to_string(),
        })?;
        drop_mirror(cx, &id)?;
        cx.write("DELETE FROM cron_jobs WHERE id = ?1", [id.as_str()])?;
        drop_identity(cx, &id)?;
        removed.push(id);
    }
    Ok(removed)
}

/// Writes a validated legacy domain into an empty cron domain inside the caller's
/// transaction; nothing is recomputed, rescheduled or rearmed.
pub(crate) fn write_import(cx: &mut WriteCx<'_>, import: &CronImport) -> Result<(), StorageError> {
    let occupied: bool = cx
        .conn
        .query_row("SELECT EXISTS (SELECT 1 FROM cron_identities)", [], |row| {
            row.get(0)
        })
        .map_err(|e| sqlite_error("inspect cron domain", e))?;
    if occupied {
        return Err(StorageError::invalid(
            "cron import",
            "requires an empty cron domain",
        ));
    }
    let revision = sql_revision(cx)?;
    let live: HashSet<&CronJobId> = import.jobs.iter().map(|job| &job.record.id).collect();
    let identities = import.jobs.iter().map(|job| (&job.record.id, false)).chain(
        import
            .histories
            .iter()
            .filter(|history| !live.contains(&history.job_id))
            .map(|history| (&history.job_id, true)),
    );
    for (id, retired) in identities {
        cx.write(
            "INSERT INTO cron_identities (job_id, revision, retired) VALUES (?1, ?2, ?3)",
            params![id.0, revision, retired],
        )?;
    }
    for history in &import.histories {
        for (ordinal, run) in (0_i64..).zip(&history.runs) {
            insert_run_row(cx, &history.job_id, run, Some(ordinal))?;
        }
    }
    for job in &import.jobs {
        let Some(last) = &job.last_run else { continue };
        let listed = import.histories.iter().any(|history| {
            history.job_id == job.record.id
                && history.runs.iter().any(|run| run.run_id == last.run_id)
        });
        if !listed {
            insert_run_row(cx, &job.record.id, last, None)?;
        }
    }
    for (ordinal, job) in (0_i64..).zip(&import.jobs) {
        insert_job_row(cx, &job.record, Some(ordinal))?;
    }
    for job in &import.jobs {
        let (Some(copy), Some(listed)) = (&job.last_run, import.divergent_row(job)) else {
            continue;
        };
        let (started_at, finished_at) = run_values(copy)?;
        let differing = CronRunField::join(&CronRunField::differing(copy, listed));
        cx.write(
            "INSERT INTO cron_last_run_mirrors (job_id, run_id, started_at, finished_at, \
             status, tab_id, session_id, cwd, summary, report_delivery, origin, \
             imported_revision, imported_differing) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'legacy_import', ?11, ?12)",
            params![
                job.record.id.0,
                copy.run_id.0,
                started_at,
                finished_at,
                copy.status.as_sql(),
                copy.tab_id,
                copy.session_id,
                copy.cwd,
                copy.summary,
                copy.report_delivery,
                revision,
                differing
            ],
        )?;
    }
    Ok(())
}
