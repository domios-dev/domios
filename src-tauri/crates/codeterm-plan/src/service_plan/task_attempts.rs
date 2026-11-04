use crate::service_plan::store_db::hot_write::AttemptTarget;
mod prompt_read;
mod prompt_render;
pub(super) mod read;
mod respawn_record;
use super::store_db::read_projections::task_drafts::TaskDraftPromptRead;
#[cfg(test)]
pub(super) use prompt_render::build_prompt;
pub(super) use prompt_render::{build_compete_prompt, build_prompt_read};
use serde::{Deserialize, Serialize};

use crate::plan::{Task, TaskExecutionAttempt, TaskLaneMetadata, TaskStatus};
use codeterm_types::agent::ReportStatus;

use super::{PlanError, PlanStore};

mod executor_identity;
mod executor_termination;
pub use executor_identity::AttemptExecutorIdentityChange;
pub use executor_termination::{ExecutorSourceFields, ExecutorSourceRead};
mod launch_binding;
mod plan_skill_evidence;
pub mod remote_dispatch;
mod remote_retirement;
mod report_ingest;
pub use report_ingest::BoundWorkerReportReceipt;
mod respawn_guard;
mod restore_binding;
mod runtime_interruption;
mod runtime_restart;
mod runtime_restore;
pub use runtime_restart::{RuntimeRestartBinding, RuntimeRestartMode};
pub use runtime_restore::RestoredRuntimeSessionBinding;

pub use launch_binding::{CompleteAcknowledgedDispatchBinding, CompletedDispatchBinding};
pub(super) use respawn_guard::ensure_respawn_source_current;
pub use restore_binding::{RestoreCurrentAttemptBinding, RestoreCurrentAttemptBindingResult};

#[cfg(test)]
mod compete_lane_tests;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskRespawnRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_profile: Option<String>,
    pub reasoning_effort: Option<String>,
    pub attempt_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub parent_tab_id: Option<String>,
    pub workspace_id: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskCompeteRequest {
    pub attempts: Option<u32>,
    pub provider: Option<String>,
    pub models: Vec<String>,
    pub parent_tab_id: Option<String>,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskAttemptView {
    #[serde(flatten)]
    pub attempt: TaskExecutionAttempt,
    pub attempt_id: String,
    pub status: String,
    pub report_command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_report: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_report_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_report_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRespawnDraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_profile: Option<String>,
    pub plan_id: String,
    pub task_id: String,
    pub execution_intent: crate::plan::ExecutionIntent,
    pub attempt_id: String,
    pub source_attempt: TaskExecutionAttempt,
    pub provider: String,
    pub planned_model: Option<String>,
    pub model: Option<String>,
    pub speed: Option<String>,
    pub reasoning_effort: Option<String>,
    pub parent_tab_id: Option<String>,
    pub workspace_id: Option<String>,
    pub title: Option<String>,
    pub explicit_title: bool,
    pub cwd: String,
    pub task_prompt: String,
    pub report_command: String,
    pub plan_skill_manifest: Vec<crate::plan::PlanSkillManifestEntry>,
    pub authorized_gates: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskCompeteDraft {
    pub plan_id: String,
    pub task_id: String,
    pub competition_group_id: String,
    pub lane: u32,
    pub provider: String,
    pub planned_model: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub parent_tab_id: Option<String>,
    pub workspace_id: Option<String>,
    pub title: Option<String>,
    pub cwd: String,
    // Filled in by the dispatcher once this competitor's own lane is provisioned;
    // `cwd` alone cannot prove isolation.
    pub branch: Option<String>,
    pub base: Option<String>,
    pub destination: Option<String>,
    pub task_prompt: String,
    pub report_command: String,
    pub plan_skill_manifest: Vec<crate::plan::PlanSkillManifestEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskAttemptCreated {
    pub plan_id: String,
    pub task_id: String,
    pub attempt: TaskExecutionAttempt,
}

#[derive(Debug, Clone)]
pub struct TaskAttemptAcknowledgement {
    pub plan_id: String,
    pub task_id: String,
    pub attempt: TaskExecutionAttempt,
    pub already_acknowledged: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BoundWorkerReport {
    pub message: String,
    pub status: Option<ReportStatus>,
    pub branch: Option<String>,
    pub sha: Option<String>,
    pub changed_files: Vec<String>,
    pub test_cmd: Option<String>,
    pub test_result: Option<String>,
    pub skipped: Option<String>,
    pub authority: Option<super::interruption::TaskMutationAuthority>,
}

fn gen_attempt_id() -> String {
    crate::plan::ids::short_id("attempt")
}

pub(super) fn report_status_key(status: ReportStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| status.label().to_ascii_lowercase().replace(' ', "_"))
}

fn split_provider_model(value: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(raw) = value.map(str::trim).filter(|s| !s.is_empty()) else {
        return (None, None);
    };
    raw.split_once('/')
        .map(|(provider, model)| (Some(provider.to_string()), Some(model.to_string())))
        .unwrap_or((None, Some(raw.to_string())))
}

pub fn task_report_command(plan_id: &str, task_id: &str) -> String {
    format!(
        "domios agent report --plan {plan_id} --task {task_id} --status in_review \"STATUS: in_review\\nCHANGES: ...\\nBUILD/TYPECHECK/TESTS: ...\\nFOLLOWUP: ...\""
    )
}

fn latest_milestone_report(
    milestones: &[crate::plan::Milestone],
    tab_id: &str,
) -> Option<(String, Option<String>, u64)> {
    milestones
        .iter()
        .filter(|milestone| milestone.from.tab_id.as_deref() == Some(tab_id))
        .max_by_key(|milestone| milestone.ts)
        .map(|milestone| {
            let status = milestone
                .text
                .lines()
                .find_map(|line| line.strip_prefix("STATUS:").map(str::trim))
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            (milestone.text.clone(), status, milestone.ts)
        })
}

fn attempt_report_status(attempt: &TaskExecutionAttempt) -> Option<String> {
    attempt
        .report
        .as_ref()
        .and_then(|report| report.status.clone())
}

fn last_report_for_milestones(
    milestones: &[crate::plan::Milestone],
    attempt: &TaskExecutionAttempt,
) -> (Option<String>, Option<String>, Option<u64>) {
    let milestone = latest_milestone_report(milestones, &attempt.executor_tab_id);
    if let Some((text, status, ts)) = milestone {
        return (
            Some(text),
            status.or_else(|| attempt_report_status(attempt)),
            Some(ts),
        );
    }
    (
        attempt
            .report
            .as_ref()
            .and_then(|report| report.message.clone()),
        attempt_report_status(attempt),
        attempt.reported_at,
    )
}

fn attempt_status(attempt: &TaskExecutionAttempt) -> String {
    if attempt.closed_at.is_some() {
        "closed".into()
    } else if attempt.reported_at.is_some() || attempt.report.is_some() {
        attempt_report_status(attempt).unwrap_or_else(|| "reported".into())
    } else if attempt.assigned_at.is_some() {
        "assigned".into()
    } else {
        "recorded".into()
    }
}

pub fn new_assignment_attempt(
    task: &Task,
    tab_id: &str,
    peer: Option<&str>,
    role: Option<&str>,
    now_ms: u64,
) -> TaskExecutionAttempt {
    new_assignment_attempt_with_id(task, tab_id, peer, role, None, now_ms)
}

pub fn new_assignment_attempt_with_id(
    task: &Task,
    tab_id: &str,
    peer: Option<&str>,
    role: Option<&str>,
    attempt_id: Option<&str>,
    now_ms: u64,
) -> TaskExecutionAttempt {
    let (provider, model) = split_provider_model(task.model.as_deref());
    TaskExecutionAttempt {
        actual_reasoning_effort: None,
        id: attempt_id
            .map(str::to_string)
            .unwrap_or_else(gen_attempt_id),
        executor_tab_id: tab_id.to_string(),
        provider: provider.clone(),
        agent_type: provider,
        provider_resolved_model: None,
        requested_model: task.model.clone(),
        model,
        role: role.map(str::to_string),
        mesh_peer: peer.map(str::to_string).or_else(|| task.peer.clone()),
        branch: task.lane.branch.clone(),
        worktree_path: task.lane.worktree_path.clone(),
        base: task.lane.base.clone(),
        destination: task.lane.destination.clone(),
        assigned_at: Some(now_ms),
        ..Default::default()
    }
}

pub fn record_noncompetitive_replacement_attempt(
    attempts: &mut Vec<TaskExecutionAttempt>,
    replacement: TaskExecutionAttempt,
    now_ms: u64,
) {
    close_live_noncompetitive_attempt_slice(attempts, now_ms);
    attempts.push(replacement);
}

fn close_live_noncompetitive_attempt_slice(
    attempts: &mut [TaskExecutionAttempt],
    now_ms: u64,
) -> usize {
    let mut closed = 0;
    for attempt in attempts {
        if attempt.closed_at.is_none() && attempt.competition_group_id.is_none() {
            attempt.closed_at = Some(now_ms);
            closed += 1;
        }
    }
    closed
}

pub fn mark_task_assignment_closed(task: &mut Task, tab: Option<&str>, now_ms: u64) -> usize {
    close_live_attempts_for_optional_tab(&mut task.execution_attempts, tab, now_ms)
}

pub fn close_all_live_attempts(attempts: &mut [TaskExecutionAttempt], now_ms: u64) -> usize {
    let mut closed = 0;
    for attempt in attempts {
        if attempt.closed_at.is_none() {
            attempt.closed_at = Some(now_ms);
            closed += 1;
        }
    }
    closed
}

pub fn close_live_attempts_for_optional_tab(
    attempts: &mut [TaskExecutionAttempt],
    tab: Option<&str>,
    now_ms: u64,
) -> usize {
    let Some(tab) = tab.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }) else {
        return 0;
    };
    close_live_attempts_for_tab(attempts, tab, now_ms)
}

pub fn close_live_attempts_for_tab(
    attempts: &mut [TaskExecutionAttempt],
    tab_id: &str,
    now_ms: u64,
) -> usize {
    let mut closed = 0;
    for attempt in attempts {
        if attempt.executor_tab_id == tab_id && attempt.closed_at.is_none() {
            attempt.closed_at = Some(now_ms);
            closed += 1;
        }
    }
    closed
}

pub fn accept_exact_attempt(attempt: &mut TaskExecutionAttempt, reason: &str, now_ms: u64) {
    let competition_winner = attempt.winner;
    crate::service_plan::competition::set_attempt_judgement(
        attempt,
        "accepted",
        Some(reason),
        competition_winner,
        now_ms,
    );
    if attempt.closed_at.is_none() {
        attempt.closed_at = Some(now_ms);
    }
}

pub(super) fn live_attempt_count(task: &Task) -> usize {
    task.execution_attempts
        .iter()
        .filter(|attempt| attempt.closed_at.is_none())
        .count()
}

pub(super) fn attempt_view(
    plan_id: &str,
    task_id: &str,
    milestones: &[crate::plan::Milestone],
    attempt: TaskExecutionAttempt,
    prompt: Option<String>,
) -> TaskAttemptView {
    let (last_report, last_report_status, last_report_at) =
        last_report_for_milestones(milestones, &attempt);
    TaskAttemptView {
        attempt_id: attempt.id.clone(),
        status: attempt_status(&attempt),
        report_command: task_report_command(plan_id, task_id),
        attempt,
        prompt,
        last_report,
        last_report_status,
        last_report_at,
    }
}

impl PlanStore {
    pub fn recent_worker_attempt_titles(
        &self,
        workspace_id: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>, PlanError> {
        self.database
            .recent_worker_titles(workspace_id, session_id, limit)
    }

    pub fn update_attempt_session_model_if_unknown(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
        model: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        let session_id = session_id.trim();
        let session_binding_nonce = session_binding_nonce.trim();
        let model = model.trim();
        if session_id.is_empty() || session_binding_nonce.is_empty() || model.is_empty() {
            return Ok(false);
        }
        self.change_attempt(
            plan_id,
            task_id,
            AttemptTarget {
                tab: tab_id,
                id: Some(attempt_id),
                session: Some((session_id, session_binding_nonce)),
                live: true,
                model_unknown: true,
                ..Default::default()
            },
            now_ms,
            |attempt| {
                let Some(attempt) = attempt.filter(|attempt| {
                    attempt.closed_at.is_none()
                        && attempt.retired_at.is_none()
                        && attempt.session_model.is_none()
                        && attempt.session_id.as_deref() == Some(session_id)
                        && attempt.session_binding_nonce.as_deref() == Some(session_binding_nonce)
                }) else {
                    return Ok(None);
                };
                attempt.session_model = Some(model.into());
                Ok(Some(true))
            },
        )
        .map(|changed| changed.unwrap_or(false))
    }

    pub fn update_attempt_model_identity(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
        requested_model: Option<String>,
        provider_resolved_model: Option<String>,
        session_model: Option<String>,
        mismatch: Option<crate::plan::AttemptModelIdentityMismatch>,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.change_attempt(
            plan_id,
            task_id,
            AttemptTarget {
                tab: tab_id,
                id: Some(attempt_id),
                session: Some((session_id, session_binding_nonce)),
                live: true,
                ..Default::default()
            },
            now_ms,
            |attempt| {
                let Some(attempt) = attempt.filter(|attempt| {
                    attempt.closed_at.is_none()
                        && attempt.retired_at.is_none()
                        && attempt.session_id.as_deref() == Some(session_id)
                        && attempt.session_binding_nonce.as_deref() == Some(session_binding_nonce)
                }) else {
                    return Ok(None);
                };
                if attempt.requested_model == requested_model
                    && attempt.provider_resolved_model == provider_resolved_model
                    && attempt.session_model == session_model
                    && attempt.model_identity_mismatch == mismatch
                {
                    return Ok(None);
                }
                attempt.requested_model = requested_model;
                attempt.provider_resolved_model = provider_resolved_model;
                attempt.session_model = session_model;
                attempt.model_identity_mismatch = mismatch;
                Ok(Some(true))
            },
        )
        .map(|changed| changed.unwrap_or(false))
    }

    pub fn update_dispatched_attempt_reasoning_effort(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        reasoning_effort: Option<String>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.change_attempt(
            plan_id,
            task_id,
            AttemptTarget {
                tab: tab_id,
                id: Some(attempt_id),
                unretired: true,
                ..Default::default()
            },
            now_ms,
            |attempt| {
                let attempt = attempt
                    .filter(|attempt| attempt.retired_at.is_none())
                    .ok_or(PlanError::TaskNotFound)?;
                if attempt.closed_at.is_some() || attempt.reasoning_effort == reasoning_effort {
                    return Ok(None);
                }
                attempt.reasoning_effort = reasoning_effort;
                Ok(Some(()))
            },
        )?;
        Ok(())
    }

    pub fn update_dispatched_attempt_actual_reasoning_effort(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
        actual_reasoning_effort: String,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.change_attempt(
            plan_id,
            task_id,
            AttemptTarget {
                tab: tab_id,
                id: Some(attempt_id),
                session: Some((session_id, session_binding_nonce)),
                live: true,
                ..Default::default()
            },
            now_ms,
            |attempt| {
                let Some(attempt) = attempt.filter(|attempt| {
                    attempt.closed_at.is_none()
                        && attempt.retired_at.is_none()
                        && attempt.session_id.as_deref() == Some(session_id)
                        && attempt.session_binding_nonce.as_deref() == Some(session_binding_nonce)
                }) else {
                    return Ok(None);
                };
                if attempt.actual_reasoning_effort.as_deref()
                    == Some(actual_reasoning_effort.as_str())
                {
                    return Ok(None);
                }
                attempt.actual_reasoning_effort = Some(actual_reasoning_effort);
                Ok(Some(true))
            },
        )
        .map(|changed| changed.unwrap_or(false))
    }

    pub fn build_task_compete_drafts(
        &self,
        plan_id: &str,
        task_id: &str,
        request: TaskCompeteRequest,
    ) -> Result<Vec<TaskCompeteDraft>, PlanError> {
        let read = self.task_draft_projection(plan_id, task_id, None)?;
        let task = &read.task;
        let (task_provider, task_model) = split_provider_model(task.model.as_deref());
        let metadata_models = task
            .competition
            .as_ref()
            .map(|competition| competition.models.clone())
            .unwrap_or_default();
        let models = if request.models.is_empty() {
            metadata_models
        } else {
            request.models.clone()
        };
        let count = request
            .attempts
            .or_else(|| {
                task.competition
                    .as_ref()
                    .and_then(|competition| competition.attempts)
            })
            .unwrap_or_else(|| models.len().max(2) as u32)
            .max(2);
        let group_id = crate::plan::ids::short_id("compete");
        let report_command = task_report_command(plan_id, &task.id);
        let manifest = super::plan_skills::plan_skill_manifest(self, plan_id)?;
        let cwd = task
            .lane
            .worktree_path
            .clone()
            .unwrap_or_else(|| read.project_path.clone());
        let mut drafts = Vec::new();
        for index in 0..count {
            let model_raw = models
                .get(index as usize)
                .cloned()
                .or_else(|| task_model.clone());
            let (model_provider, model_name) = split_provider_model(model_raw.as_deref());
            let provider = model_provider
                .or_else(|| request.provider.clone())
                .or_else(|| task_provider.clone())
                .unwrap_or_else(|| "claude".into());
            let model = model_name.or(model_raw);
            let model_label = model
                .as_deref()
                .map(|m| format!("{provider}/{m}"))
                .unwrap_or_else(|| provider.clone());
            let lane = index + 1;
            let task_prompt = build_compete_prompt(
                plan_id,
                task,
                &group_id,
                lane,
                &model_label,
                &report_command,
                &manifest,
            );
            drafts.push(TaskCompeteDraft {
                plan_id: read.plan_id.clone(),
                task_id: task.id.clone(),
                competition_group_id: group_id.clone(),
                lane,
                provider,
                planned_model: task.model.clone(),
                model,
                reasoning_effort: None,
                parent_tab_id: request.parent_tab_id.clone().or(read.owner_tab_id.clone()),
                workspace_id: request
                    .workspace_id
                    .clone()
                    .or(task.lane.workspace_id.clone()),
                title: None,
                cwd: cwd.clone(),
                branch: None,
                base: None,
                destination: task.lane.destination.clone(),
                task_prompt,
                report_command: report_command.clone(),
                plan_skill_manifest: manifest.clone(),
            });
        }
        Ok(drafts)
    }

    pub fn list_task_attempts(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Vec<TaskAttemptView>, PlanError> {
        let task = self
            .database
            .task_attempt_projection(plan_id, task_id, None)?;
        Ok(task
            .execution_attempts
            .iter()
            .cloned()
            .map(|attempt| attempt_view(plan_id, &task.id, &task.milestones, attempt, None))
            .collect())
    }

    pub fn show_task_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
    ) -> Result<TaskAttemptView, PlanError> {
        let task = self
            .database
            .task_attempt_projection(plan_id, task_id, Some(attempt_id))?;
        let source = task.execution_attempts[0].clone();
        let prompt_input = task
            .prompt
            .as_ref()
            .ok_or_else(|| PlanError::Io("selected attempt has no prompt facts".into()))?;
        let (task_provider, task_model) = split_provider_model(prompt_input.model.as_deref());
        let provider = source
            .provider
            .clone()
            .or(task_provider)
            .unwrap_or_else(|| "claude".into());
        let model = source.model.clone().or(task_model);
        let report_command = task_report_command(plan_id, &task.id);
        let prompt = build_prompt_read(
            plan_id,
            (prompt_input, task.milestones.as_slice()),
            &source,
            &provider,
            model.as_deref(),
            &report_command,
            &source.plan_skill_manifest,
            source.authorized_gates.as_deref().unwrap_or(&[]),
        );
        Ok(attempt_view(
            plan_id,
            &task.id,
            &task.milestones,
            source,
            Some(prompt),
        ))
    }

    pub fn build_task_respawn_draft(
        &self,
        plan_id: &str,
        task_id: &str,
        request: TaskRespawnRequest,
    ) -> Result<TaskRespawnDraft, PlanError> {
        let mut read = self.task_draft_projection(plan_id, task_id, Some(&request.attempt_id))?;
        let task = &read.task;
        super::task_dispatch::task_respawn_status(&task.status)?;
        let source = read.source.take().ok_or(PlanError::TaskNotFound)?;
        let (task_provider, task_model) = split_provider_model(task.model.as_deref());
        let source_provider = source.provider.clone().or(source.agent_type.clone());
        let provider = request
            .provider
            .or(source_provider.clone())
            .or(task_provider)
            .unwrap_or_else(|| "claude".into());
        let model = request
            .model
            .or(source.model.clone())
            .or(source.provider_resolved_model.clone())
            .or(source.requested_model.clone())
            .or_else(|| source_provider.is_none().then_some(task_model).flatten());
        let reasoning_effort = request.reasoning_effort.or(source.reasoning_effort.clone());
        let report_command = task_report_command(plan_id, &task.id);
        let manifest = super::plan_skills::plan_skill_manifest(self, plan_id)?;
        let explicit_title = request.title.is_some();
        let title = request.title.or_else(|| task.personality.clone());
        let cwd = source
            .worktree_path
            .clone()
            .or_else(|| task.lane.worktree_path.clone())
            .unwrap_or_else(|| read.project_path.clone());
        let task_prompt = build_prompt_read(
            plan_id,
            task,
            &source,
            &provider,
            model.as_deref(),
            &report_command,
            &manifest,
            source.authorized_gates.as_deref().unwrap_or(&[]),
        );
        Ok(TaskRespawnDraft {
            launch_profile: request.launch_profile.or_else(|| {
                (source_provider.as_deref() == Some(provider.as_str()))
                    .then(|| source.launch_profile.clone())
                    .flatten()
            }),
            plan_id: read.plan_id,
            task_id: task.id.clone(),
            execution_intent: task.execution_intent.clone(),
            attempt_id: gen_attempt_id(),
            speed: task.speed.clone(),
            source_attempt: source.clone(),
            provider,
            planned_model: task.model.clone(),
            model,
            reasoning_effort,
            parent_tab_id: request
                .parent_tab_id
                .or(source.parent_tab_id.clone())
                .or(read.owner_tab_id),
            workspace_id: request.workspace_id.or(task.lane.workspace_id.clone()),
            title,
            explicit_title,
            cwd,
            task_prompt,
            report_command,
            plan_skill_manifest: manifest,
            authorized_gates: source.authorized_gates.clone(),
        })
    }

    pub fn record_task_compete_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        draft: &TaskCompeteDraft,
        role: Option<&str>,
        now_ms: u64,
    ) -> Result<TaskAttemptCreated, PlanError> {
        let mut created = None;
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::LiveAndReported,
            now_ms,
            |edit| {
                edit.warn_dispatch = edit.task.status == TaskStatus::Pending;
                let task = &mut edit.task;
                task.status_label = Some("competitive".into());
                task.updated_at = now_ms;
                let mut attempt = new_assignment_attempt(task, tab_id, None, role, now_ms);
                attempt.provider = Some(draft.provider.clone());
                attempt.agent_type = Some(draft.provider.clone());
                attempt.model = draft.model.clone();
                attempt.provider_resolved_model = draft.model.clone();
                attempt.reasoning_effort = draft.reasoning_effort.clone();
                attempt.executor_title = draft.title.clone();
                attempt.parent_tab_id = draft.parent_tab_id.clone();
                attempt.competition_group_id = Some(draft.competition_group_id.clone());
                attempt.competition_lane = Some(draft.lane);
                attempt.plan_skill_manifest = draft.plan_skill_manifest.clone();
                if draft.branch.is_some() {
                    attempt.worktree_path = Some(draft.cwd.clone());
                    attempt.branch = draft.branch.clone();
                    attempt.base = draft.base.clone();
                    attempt.destination = draft.destination.clone();
                }
                task.execution_attempts.push(attempt.clone());
                created = Some(attempt);
                Ok(())
            },
        )?;
        Ok(TaskAttemptCreated {
            plan_id: plan_id.into(),
            task_id: task_id.into(),
            attempt: created.expect("attempt set before successful write"),
        })
    }

    pub fn judge_task_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        verdict: &str,
        reason: Option<&str>,
        winner: bool,
        now_ms: u64,
    ) -> Result<TaskAttemptCreated, PlanError> {
        let mut judged = None;
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                crate::service_plan::competition::set_attempt_judgement(
                    attempt, verdict, reason, winner, now_ms,
                );
                judged = Some(attempt.clone());
                if winner {
                    task.status = TaskStatus::InReview;
                    task.status_label = Some("judging".into());
                    task.status_changed_at = now_ms;
                }
                task.updated_at = now_ms;
                Ok(())
            },
        )?;
        Ok(TaskAttemptCreated {
            plan_id: plan_id.into(),
            task_id: task_id.into(),
            attempt: judged.expect("attempt set before successful write"),
        })
    }
}
