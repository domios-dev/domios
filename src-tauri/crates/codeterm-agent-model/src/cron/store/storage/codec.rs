use super::super::{CronJob, CronRunInfo, CronRunStatus, CronScope};
use crate::storage::{
    CronJobId, CronJobRecord, CronRunId, CronRunRecord, CronRunState, StorageError, StoredCronJob,
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
        cancel_reason: run.cancel_reason,
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
        cancel_reason: run.cancel_reason,
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
