//! Typed cron rows, write batches and import sets, with their validation and bounds.

use crate::error::StorageError;
use crate::repositories::{to_sql_u64, Revision};
use std::collections::HashSet;

pub const MAX_CRON_JOBS: usize = 4096;
pub const MAX_CRON_HISTORY: u64 = 10_000;
/// Writes in one batch; a batch is one transaction.
pub const MAX_CRON_BATCH: usize = 64;
const MAX_ID_BYTES: usize = 256;
const MAX_LABEL_BYTES: usize = 4 * 1024;
const MAX_PATH_BYTES: usize = 32 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const RECORD_OVERHEAD: usize = 128;

fn identifier(field: &'static str, value: String) -> Result<String, StorageError> {
    if value.is_empty() || value.len() > MAX_ID_BYTES || value.chars().any(char::is_control) {
        return Err(StorageError::invalid(
            field,
            "must be 1..=256 bytes without control characters",
        ));
    }
    Ok(value)
}

/// Identity of one cron job; it stays reserved after the definition is deleted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CronJobId(pub(crate) String);

impl CronJobId {
    pub fn new(id: impl Into<String>) -> Result<Self, StorageError> {
        identifier("cron job id", id.into()).map(CronJobId)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CronRunId(pub(crate) String);

impl CronRunId {
    pub fn new(id: impl Into<String>) -> Result<Self, StorageError> {
        identifier("cron run id", id.into()).map(CronRunId)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronRunState {
    Running,
    AwaitingOwnerPrompt,
    Success,
    Failed,
    Cancelled,
}

impl CronRunState {
    pub(crate) fn as_sql(self) -> &'static str {
        match self {
            CronRunState::Running => "running",
            CronRunState::AwaitingOwnerPrompt => "awaiting_owner_prompt",
            CronRunState::Success => "success",
            CronRunState::Failed => "failed",
            CronRunState::Cancelled => "cancelled",
        }
    }

    pub(crate) fn from_sql(text: &str) -> Result<Self, StorageError> {
        Ok(match text {
            "running" => CronRunState::Running,
            "awaiting_owner_prompt" => CronRunState::AwaitingOwnerPrompt,
            "success" => CronRunState::Success,
            "failed" => CronRunState::Failed,
            "cancelled" => CronRunState::Cancelled,
            _ => {
                return Err(StorageError::Corrupt {
                    detail: "unknown cron run status".to_string(),
                })
            }
        })
    }
}

/// One run row; the same row backs a history entry and a job's last run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronRunRecord {
    pub run_id: CronRunId,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: CronRunState,
    pub tab_id: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub summary: Option<String>,
    pub report_delivery: Option<String>,
}

impl CronRunRecord {
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        to_sql_u64("run start", self.started_at)?;
        if let Some(finished) = self.finished_at {
            to_sql_u64("run finish", finished)?;
        }
        optional("run tab", &self.tab_id, MAX_LABEL_BYTES)?;
        optional("run session", &self.session_id, MAX_LABEL_BYTES)?;
        optional("run cwd", &self.cwd, MAX_PATH_BYTES)?;
        optional("run summary", &self.summary, MAX_TEXT_BYTES)?;
        optional("run report delivery", &self.report_delivery, MAX_TEXT_BYTES)
    }

    fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + self.run_id.0.len()
            + [
                &self.tab_id,
                &self.session_id,
                &self.cwd,
                &self.summary,
                &self.report_delivery,
            ]
            .into_iter()
            .map(text_len)
            .sum::<usize>()
    }
}

/// Every scalar of one live cron definition; `last_run_id` references a run row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJobRecord {
    pub id: CronJobId,
    pub name: String,
    pub description: Option<String>,
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub schedule: String,
    pub enabled: bool,
    pub agent: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub task: String,
    pub cwd: Option<String>,
    pub role: Option<String>,
    pub skill: Option<String>,
    pub creator_tab_id: Option<String>,
    pub report_target_tab_id: Option<String>,
    pub oneshot: bool,
    pub max_history: u64,
    pub created_at: u64,
    pub last_run_id: Option<CronRunId>,
    pub next_run: Option<u64>,
    pub jitter_max_ms: Option<u64>,
    pub tz: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub consecutive_failures: u32,
    pub protected: bool,
}

impl CronJobRecord {
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        for (field, value) in [
            ("cron name", &self.name),
            ("cron workspace", &self.workspace_id),
            ("cron schedule", &self.schedule),
            ("cron agent", &self.agent),
        ] {
            bounded(field, value, MAX_LABEL_BYTES)?;
        }
        bounded("cron task", &self.task, MAX_TEXT_BYTES)?;
        optional("cron description", &self.description, MAX_TEXT_BYTES)?;
        optional("cron cwd", &self.cwd, MAX_PATH_BYTES)?;
        for (field, value) in [
            ("cron team", &self.team_id),
            ("cron model", &self.model),
            ("cron reasoning effort", &self.reasoning_effort),
            ("cron role", &self.role),
            ("cron skill", &self.skill),
            ("cron creator tab", &self.creator_tab_id),
            ("cron report target", &self.report_target_tab_id),
            ("cron timezone", &self.tz),
        ] {
            optional(field, value, MAX_LABEL_BYTES)?;
        }
        if self.max_history > MAX_CRON_HISTORY {
            return Err(StorageError::invalid(
                "cron max history",
                format!("exceeds {MAX_CRON_HISTORY}"),
            ));
        }
        to_sql_u64("cron created at", self.created_at)?;
        for (field, value) in [
            ("cron next run", self.next_run),
            ("cron jitter", self.jitter_max_ms),
            ("cron timeout", self.timeout_seconds),
        ] {
            if let Some(value) = value {
                to_sql_u64(field, value)?;
            }
        }
        Ok(())
    }

    fn cost(&self) -> usize {
        let optional = [
            &self.description,
            &self.team_id,
            &self.model,
            &self.reasoning_effort,
            &self.cwd,
            &self.role,
            &self.skill,
            &self.creator_tab_id,
            &self.report_target_tab_id,
            &self.tz,
        ]
        .into_iter()
        .map(text_len)
        .sum::<usize>();
        RECORD_OVERHEAD
            + self.id.0.len()
            + self.name.len()
            + self.workspace_id.len()
            + self.schedule.len()
            + self.agent.len()
            + self.task.len()
            + optional
    }
}

fn text_len(value: &Option<String>) -> usize {
    value.as_ref().map_or(0, String::len)
}

fn bounded(field: &'static str, value: &str, limit: usize) -> Result<(), StorageError> {
    if value.len() > limit {
        return Err(StorageError::invalid(
            field,
            format!("exceeds {limit} bytes"),
        ));
    }
    Ok(())
}

fn optional(field: &'static str, value: &Option<String>, limit: usize) -> Result<(), StorageError> {
    value
        .as_deref()
        .map_or(Ok(()), |value| bounded(field, value, limit))
}

/// One field of a run, as named in divergence metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CronRunField {
    StartedAt,
    FinishedAt,
    Status,
    TabId,
    SessionId,
    Cwd,
    Summary,
    ReportDelivery,
}

impl CronRunField {
    const ALL: [CronRunField; 8] = [
        CronRunField::StartedAt,
        CronRunField::FinishedAt,
        CronRunField::Status,
        CronRunField::TabId,
        CronRunField::SessionId,
        CronRunField::Cwd,
        CronRunField::Summary,
        CronRunField::ReportDelivery,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CronRunField::StartedAt => "started_at",
            CronRunField::FinishedAt => "finished_at",
            CronRunField::Status => "status",
            CronRunField::TabId => "tab_id",
            CronRunField::SessionId => "session_id",
            CronRunField::Cwd => "cwd",
            CronRunField::Summary => "summary",
            CronRunField::ReportDelivery => "report_delivery",
        }
    }

    pub(crate) fn parse_list(text: &str) -> Result<Vec<CronRunField>, StorageError> {
        text.split(',')
            .map(|name| {
                CronRunField::ALL
                    .into_iter()
                    .find(|field| field.as_str() == name)
                    .ok_or_else(|| StorageError::Corrupt {
                        detail: "unknown cron run field".to_string(),
                    })
            })
            .collect()
    }

    pub(crate) fn join(fields: &[CronRunField]) -> String {
        let names: Vec<&str> = fields.iter().map(|field| field.as_str()).collect();
        names.join(",")
    }

    /// Fields whose values differ between two copies of the same run, in a fixed order.
    pub fn differing(left: &CronRunRecord, right: &CronRunRecord) -> Vec<CronRunField> {
        let differs = |field: CronRunField| match field {
            CronRunField::StartedAt => left.started_at != right.started_at,
            CronRunField::FinishedAt => left.finished_at != right.finished_at,
            CronRunField::Status => left.status != right.status,
            CronRunField::TabId => left.tab_id != right.tab_id,
            CronRunField::SessionId => left.session_id != right.session_id,
            CronRunField::Cwd => left.cwd != right.cwd,
            CronRunField::Summary => left.summary != right.summary,
            CronRunField::ReportDelivery => left.report_delivery != right.report_delivery,
        };
        CronRunField::ALL
            .into_iter()
            .filter(|field| differs(*field))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronDivergenceOrigin {
    /// An original source stored a last run that differs from its own history row.
    LegacyImport,
}

/// Provenance of a last run kept apart from its history row. The copy itself is current
/// state that later run updates evolve; `imported_differing` is fixed at import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronLastRunDivergence {
    pub origin: CronDivergenceOrigin,
    pub imported_revision: Revision,
    pub imported_differing: Vec<CronRunField>,
}

/// A live definition with its resolved last run; with a divergence, `last_run` is the
/// job's own copy and the history row with the same run id holds the other copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCronJob {
    pub record: CronJobRecord,
    pub last_run: Option<CronRunRecord>,
    pub divergence: Option<CronLastRunDivergence>,
}

/// Revision covers the definition and every run row of the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CronIdentity {
    pub revision: Revision,
    pub retired: bool,
}

/// Everything one identity owns: its live definition (none once retired) and its history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJobState {
    pub job_id: CronJobId,
    pub identity: CronIdentity,
    pub job: Option<StoredCronJob>,
    pub history: Vec<CronRunRecord>,
}

/// Live definitions in definition order, then retired identities ordered by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSnapshot {
    pub states: Vec<CronJobState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronHistoryDisposition {
    Keep,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronScope {
    Workspace(String),
    Team(String),
}

/// One incremental write; every identity-scoped write names its expected revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronWrite {
    Create(CronJobRecord),
    Patch {
        job: CronJobRecord,
        expect: Revision,
    },
    /// Appends the run as the job's last run and trims history to `max_history`.
    StartRun {
        job: CronJobRecord,
        run: CronRunRecord,
        expect: Revision,
    },
    /// Rewrites existing run rows; `job` also rewrites the definition, never its last run.
    /// `last_run` evolves an imported divergent copy; it never creates one.
    UpdateRuns {
        job_id: CronJobId,
        expect: Revision,
        runs: Vec<CronRunRecord>,
        job: Option<CronJobRecord>,
        last_run: Option<CronRunRecord>,
    },
    Remove {
        id: CronJobId,
        expect: Revision,
        history: CronHistoryDisposition,
    },
    /// Removes every live job of the scope with its history.
    RemoveScope(CronScope),
}

impl CronWrite {
    fn identity(&self) -> Option<&CronJobId> {
        match self {
            CronWrite::Create(job)
            | CronWrite::Patch { job, .. }
            | CronWrite::StartRun { job, .. } => Some(&job.id),
            CronWrite::UpdateRuns { job_id, .. } => Some(job_id),
            CronWrite::Remove { id, .. } => Some(id),
            CronWrite::RemoveScope(_) => None,
        }
    }

    fn cost(&self) -> usize {
        match self {
            CronWrite::Create(job) | CronWrite::Patch { job, .. } => job.cost(),
            CronWrite::StartRun { job, run, .. } => job.cost() + run.cost(),
            CronWrite::UpdateRuns {
                runs,
                job,
                last_run,
                ..
            } => {
                RECORD_OVERHEAD
                    + runs.iter().map(CronRunRecord::cost).sum::<usize>()
                    + job.as_ref().map_or(0, CronJobRecord::cost)
                    + last_run.as_ref().map_or(0, CronRunRecord::cost)
            }
            CronWrite::Remove { id, .. } => RECORD_OVERHEAD + id.0.len(),
            CronWrite::RemoveScope(CronScope::Workspace(scope) | CronScope::Team(scope)) => {
                RECORD_OVERHEAD + scope.len()
            }
        }
    }

    fn validate(&self) -> Result<(), StorageError> {
        match self {
            CronWrite::Create(job) | CronWrite::Patch { job, .. } => job.validate(),
            CronWrite::StartRun { job, run, .. } => {
                job.validate()?;
                run.validate()?;
                if job.last_run_id.as_ref() != Some(&run.run_id) {
                    return Err(StorageError::invalid(
                        "cron run start",
                        "the started run must become the job's last run",
                    ));
                }
                Ok(())
            }
            CronWrite::UpdateRuns {
                job_id,
                runs,
                job,
                last_run,
                ..
            } => {
                if let Some(last_run) = last_run {
                    last_run.validate()?;
                }
                let mut seen = HashSet::new();
                for run in runs {
                    run.validate()?;
                    if !seen.insert(&run.run_id) {
                        return Err(StorageError::invalid("cron run update", "repeats a run id"));
                    }
                }
                match job {
                    Some(job) if job.id != *job_id => Err(StorageError::invalid(
                        "cron run update",
                        "the definition belongs to another job",
                    )),
                    Some(job) => job.validate(),
                    None => Ok(()),
                }
            }
            CronWrite::Remove { .. } | CronWrite::RemoveScope(_) => Ok(()),
        }
    }
}

/// Validates a batch and returns its mailbox cost; each identity appears at most once.
pub(crate) fn validate_batch(batch: &[CronWrite]) -> Result<usize, StorageError> {
    if batch.is_empty() || batch.len() > MAX_CRON_BATCH {
        return Err(StorageError::invalid(
            "cron batch",
            format!("must hold 1..={MAX_CRON_BATCH} writes"),
        ));
    }
    let mut seen = HashSet::new();
    for write in batch {
        write.validate()?;
        if let Some(id) = write.identity() {
            if !seen.insert(id) {
                return Err(StorageError::invalid(
                    "cron batch",
                    "repeats a job identity",
                ));
            }
        }
    }
    Ok(batch.iter().map(CronWrite::cost).sum())
}

/// Live jobs removed by a committed batch, in removal order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CronApplied {
    pub removed: Vec<CronJobId>,
}

/// A definition as decoded from a legacy source, with its own last run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJobImport {
    pub record: CronJobRecord,
    pub last_run: Option<CronRunRecord>,
}

/// One legacy history list; a key without a live definition becomes a retired identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronHistoryImport {
    pub job_id: CronJobId,
    pub runs: Vec<CronRunRecord>,
}

/// A complete cron domain read from an original source, conserved without recomputation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CronImport {
    pub jobs: Vec<CronJobImport>,
    pub histories: Vec<CronHistoryImport>,
}

impl CronImport {
    /// Refuses, with the exact identities involved, anything that cannot be stored as given.
    pub fn validate(&self) -> Result<usize, StorageError> {
        if self.jobs.len() > MAX_CRON_JOBS {
            return Err(StorageError::invalid(
                "cron import",
                "exceeds the definition bound",
            ));
        }
        let mut live = HashSet::new();
        let mut cost = 0;
        for job in &self.jobs {
            job.record.validate()?;
            if !live.insert(&job.record.id) {
                return Err(StorageError::invalid(
                    "cron import",
                    format!("repeats job {}", job.record.id.0),
                ));
            }
            if job.record.last_run_id.as_ref() != job.last_run.as_ref().map(|run| &run.run_id) {
                return Err(StorageError::invalid(
                    "cron import",
                    format!("job {} names a last run it does not carry", job.record.id.0),
                ));
            }
            if let Some(run) = &job.last_run {
                run.validate()?;
                cost += run.cost();
            }
            cost += job.record.cost();
        }
        let mut keys = HashSet::new();
        for history in &self.histories {
            if !keys.insert(&history.job_id) {
                return Err(StorageError::invalid(
                    "cron import",
                    format!("repeats history of job {}", history.job_id.0),
                ));
            }
            if history.runs.len() as u64 > MAX_CRON_HISTORY {
                return Err(StorageError::invalid(
                    "cron import",
                    format!(
                        "history of job {} exceeds {MAX_CRON_HISTORY}",
                        history.job_id.0
                    ),
                ));
            }
            let mut runs = HashSet::new();
            for run in &history.runs {
                run.validate()?;
                if !runs.insert(&run.run_id) {
                    return Err(StorageError::invalid(
                        "cron import",
                        format!("job {} repeats run {}", history.job_id.0, run.run_id.0),
                    ));
                }
                cost += run.cost();
            }
        }
        Ok(cost)
    }

    /// The history row an imported last run shares its run id with, when they differ.
    pub(crate) fn divergent_row<'a>(&'a self, job: &'a CronJobImport) -> Option<&'a CronRunRecord> {
        let last = job.last_run.as_ref()?;
        self.histories
            .iter()
            .find(|history| history.job_id == job.record.id)?
            .runs
            .iter()
            .find(|run| run.run_id == last.run_id)
            .filter(|listed| *listed != last)
    }
}
