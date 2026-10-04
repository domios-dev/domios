//! Conversions between cache values and Storage records, and the read-only decoder of
//! an original crons.json that never rewrites it.

use std::fmt;
use std::path::Path;

use super::super::{CronFile, CronJob, CronRunInfo, CronRunStatus, CronScope};
use crate::storage::{
    CronHistoryImport, CronImport, CronJobId, CronJobImport, CronJobRecord, CronRunId,
    CronRunRecord, CronRunState, StorageError, StoredCronJob,
};

pub(super) fn stored_scope(scope: &CronScope) -> crate::storage::CronScope {
    match scope {
        CronScope::Workspace(id) => crate::storage::CronScope::Workspace(id.clone()),
        CronScope::Team(id) => crate::storage::CronScope::Team(id.clone()),
    }
}

/// Definition values without the last-run mirror, which lives in its own row.
pub(super) fn definition_of(job: &CronJob) -> CronJob {
    CronJob {
        last_run: None,
        ..job.clone()
    }
}

pub(super) fn state_of(status: &CronRunStatus) -> CronRunState {
    match status {
        CronRunStatus::Running => CronRunState::Running,
        CronRunStatus::AwaitingOwnerPrompt => CronRunState::AwaitingOwnerPrompt,
        CronRunStatus::Success => CronRunState::Success,
        CronRunStatus::Failed => CronRunState::Failed,
        CronRunStatus::Cancelled => CronRunState::Cancelled,
    }
}

pub(super) fn status_of(state: CronRunState) -> CronRunStatus {
    match state {
        CronRunState::Running => CronRunStatus::Running,
        CronRunState::AwaitingOwnerPrompt => CronRunStatus::AwaitingOwnerPrompt,
        CronRunState::Success => CronRunStatus::Success,
        CronRunState::Failed => CronRunStatus::Failed,
        CronRunState::Cancelled => CronRunStatus::Cancelled,
    }
}

pub(super) fn run_record(run: &CronRunInfo) -> Result<CronRunRecord, StorageError> {
    Ok(CronRunRecord {
        run_id: CronRunId::new(run.run_id.clone())?,
        started_at: run.started_at,
        finished_at: run.finished_at,
        status: state_of(&run.status),
        tab_id: run.tab_id.clone(),
        session_id: run.session_id.clone(),
        cwd: run.cwd.clone(),
        summary: run.summary.clone(),
        report_delivery: run.report_delivery.clone(),
    })
}

pub(super) fn run_info(run: CronRunRecord) -> CronRunInfo {
    CronRunInfo {
        run_id: run.run_id.as_str().to_string(),
        started_at: run.started_at,
        finished_at: run.finished_at,
        status: status_of(run.status),
        tab_id: run.tab_id,
        session_id: run.session_id,
        cwd: run.cwd,
        summary: run.summary,
        report_delivery: run.report_delivery,
    }
}

pub(super) fn job_record(job: &CronJob) -> Result<CronJobRecord, StorageError> {
    Ok(CronJobRecord {
        id: CronJobId::new(job.id.clone())?,
        name: job.name.clone(),
        description: job.description.clone(),
        workspace_id: job.workspace_id.clone(),
        team_id: job.team_id.clone(),
        schedule: job.schedule.clone(),
        enabled: job.enabled,
        agent: job.agent.clone(),
        model: job.model.clone(),
        reasoning_effort: job.reasoning_effort.clone(),
        task: job.task.clone(),
        cwd: job.cwd.clone(),
        role: job.role.clone(),
        skill: job.skill.clone(),
        creator_tab_id: job.creator_tab_id.clone(),
        report_target_tab_id: job.report_target_tab_id.clone(),
        oneshot: job.oneshot,
        max_history: u64::try_from(job.max_history).map_err(|_| StorageError::Invalid {
            field: "cron max history",
            reason: "exceeds the 64-bit range".to_string(),
        })?,
        created_at: job.created_at,
        last_run_id: job
            .last_run
            .as_ref()
            .map(|run| CronRunId::new(run.run_id.clone()))
            .transpose()?,
        next_run: job.next_run,
        jitter_max_ms: job.jitter_max_ms,
        tz: job.tz.clone(),
        timeout_seconds: job.timeout_seconds,
        consecutive_failures: job.consecutive_failures,
        protected: job.protected,
    })
}

/// The cache keeps a divergent copy as `last_run`; its provenance stays in Storage.
pub(super) fn job_info(stored: StoredCronJob) -> Result<CronJob, String> {
    let StoredCronJob {
        record, last_run, ..
    } = stored;
    Ok(CronJob {
        id: record.id.as_str().to_string(),
        name: record.name,
        description: record.description,
        workspace_id: record.workspace_id,
        team_id: record.team_id,
        schedule: record.schedule,
        enabled: record.enabled,
        agent: record.agent,
        model: record.model,
        reasoning_effort: record.reasoning_effort,
        task: record.task,
        cwd: record.cwd,
        role: record.role,
        skill: record.skill,
        creator_tab_id: record.creator_tab_id,
        report_target_tab_id: record.report_target_tab_id,
        oneshot: record.oneshot,
        max_history: usize::try_from(record.max_history)
            .map_err(|_| "cron max history exceeds this platform".to_string())?,
        created_at: record.created_at,
        last_run: last_run.map(run_info),
        next_run: record.next_run,
        jitter_max_ms: record.jitter_max_ms,
        tz: record.tz,
        timeout_seconds: record.timeout_seconds,
        consecutive_failures: record.consecutive_failures,
        protected: record.protected,
    })
}

/// Why an original crons.json cannot be imported; a genuinely absent file is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyCronError {
    Unreadable(String),
    Corrupt(String),
    Unsupported(String),
    /// The original holds facts that cannot be stored as given.
    Conflict(String),
}

impl fmt::Display for LegacyCronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LegacyCronError::Unreadable(detail) => write!(f, "crons.json unreadable: {detail}"),
            LegacyCronError::Corrupt(detail) => write!(f, "crons.json corrupt: {detail}"),
            LegacyCronError::Unsupported(detail) => write!(f, "crons.json unsupported: {detail}"),
            LegacyCronError::Conflict(detail) => write!(f, "crons.json conflict: {detail}"),
        }
    }
}

/// Reads the original without recovery, quarantine or rewrite; `Ok(None)` only when the
/// file is genuinely absent.
pub fn read_legacy_cron_file(path: &Path) -> Result<Option<CronImport>, LegacyCronError> {
    match std::fs::read(path) {
        Ok(bytes) => decode_legacy_cron_file(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(LegacyCronError::Unreadable(error.to_string())),
    }
}

/// Decodes original bytes into a conserved import: definition order, each history list
/// in order, nulls and the last-run mirror as stored. Nothing is rescheduled.
pub fn decode_legacy_cron_file(bytes: &[u8]) -> Result<CronImport, LegacyCronError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| LegacyCronError::Corrupt(error.to_string()))?;
    match value.get("version").and_then(serde_json::Value::as_str) {
        Some("1") => {}
        Some(other) => return Err(LegacyCronError::Unsupported(format!("version {other}"))),
        None => return Err(LegacyCronError::Corrupt("missing version".to_string())),
    }
    let file: CronFile = serde_json::from_value(value)
        .map_err(|error| LegacyCronError::Corrupt(error.to_string()))?;
    let conflict = |error: StorageError| LegacyCronError::Conflict(error.to_string());
    let mut jobs = Vec::with_capacity(file.jobs.len());
    for job in &file.jobs {
        jobs.push(CronJobImport {
            record: job_record(job).map_err(conflict)?,
            last_run: job
                .last_run
                .as_ref()
                .map(run_record)
                .transpose()
                .map_err(conflict)?,
        });
    }
    let mut histories = Vec::with_capacity(file.history.len());
    for (job_id, runs) in &file.history {
        histories.push(CronHistoryImport {
            job_id: CronJobId::new(job_id.clone()).map_err(conflict)?,
            runs: runs
                .iter()
                .map(run_record)
                .collect::<Result<_, _>>()
                .map_err(conflict)?,
        });
    }
    // History is a map: sorting by identity makes the import deterministic.
    histories.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    let import = CronImport { jobs, histories };
    import.validate().map_err(conflict)?;
    Ok(import)
}
