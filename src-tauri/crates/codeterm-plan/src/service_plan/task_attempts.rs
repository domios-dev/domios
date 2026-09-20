use serde::{Deserialize, Serialize};

use crate::plan::{
    AgentRef, Milestone, Task, TaskExecutionAttempt, TaskExecutionReportSnapshot, TaskLaneMetadata,
    TaskStatus,
};
use codeterm_types::agent::ReportStatus;

use super::{PlanError, PlanStore};

mod plan_skill_evidence;

#[cfg(test)]
mod compete_lane_tests;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskRespawnRequest {
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
    pub plan_id: String,
    pub task_id: String,
    pub source_attempt: TaskExecutionAttempt,
    pub provider: String,
    pub planned_model: Option<String>,
    pub model: Option<String>,
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

fn report_status_key(status: ReportStatus) -> String {
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
        "codeterm agent report --plan {plan_id} --task {task_id} --status in_review \"STATUS: in_review\\nCHANGES: ...\\nBUILD/TYPECHECK/TESTS: ...\\nFOLLOWUP: ...\""
    )
}

fn latest_milestone_report(task: &Task, tab_id: &str) -> Option<(String, Option<String>, u64)> {
    task.milestones
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

fn last_report_for_attempt(
    task: &Task,
    attempt: &TaskExecutionAttempt,
) -> (Option<String>, Option<String>, Option<u64>) {
    let milestone = latest_milestone_report(task, &attempt.executor_tab_id);
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

fn lane_lines(lane: &TaskLaneMetadata) -> Vec<String> {
    [
        ("worktree", lane.worktree_path.as_deref()),
        ("branch", lane.branch.as_deref()),
        ("base", lane.base.as_deref()),
        ("destination", lane.destination.as_deref()),
        ("workspace", lane.workspace_id.as_deref()),
        ("workspace_name", lane.workspace_name.as_deref()),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.map(|v| format!("- {name}: {v}")))
    .collect()
}

fn build_prompt(
    plan_id: &str,
    task: &Task,
    source: &TaskExecutionAttempt,
    provider: &str,
    model: Option<&str>,
    report_command: &str,
    manifest: &[crate::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    let identity =
        super::worker_identity::compose_task_worker_identity(task, task.personality.as_deref());
    let (last_report, last_status, _) = last_report_for_attempt(task, source);
    let mut out = String::new();
    out.push_str("You are a CodeTerm Worker respawned from a prior task attempt.\n");
    out.push_str(
        "Work only on this one task. Do not reactivate or reuse the old historical attempt.\n\n",
    );
    out.push_str(&format!("Plan: {plan_id}\nTask: {}\n", task.id));
    out.push_str(&super::task_brief::render_plan_skill_manifest(
        plan_id, manifest,
    ));
    out.push_str(&super::task_brief::render_authorized_gates(
        authorized_gates,
    ));
    out.push_str(&format!("Task name: {}\n", task.name));
    out.push_str(&format!("Provider/model preference: {provider}"));
    if let Some(model) = model {
        out.push_str(&format!("/{model}"));
    }
    out.push('\n');
    if let Some(strides) = super::stride_context::render_worker_strides(&task.milestones) {
        out.push('\n');
        out.push_str(&strides);
    }
    out.push_str("\nOriginal task body:\n");
    if task.body.trim().is_empty() {
        out.push_str("(no body recorded)\n");
    } else {
        out.push_str(task.body.trim());
        out.push('\n');
    }
    if let Some(intent) = task.intent.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("\nIntent:\n");
        out.push_str(intent.trim());
        out.push('\n');
    }
    if !task.acceptance_criteria.is_empty() {
        out.push_str("\nAcceptance criteria:\n");
        for criterion in &task.acceptance_criteria {
            out.push_str("- ");
            out.push_str(criterion);
            out.push('\n');
        }
    }
    if let Some(correctness) = task
        .correctness
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("\nCorrectness:\n");
        out.push_str(correctness);
        out.push('\n');
    }
    if let Some(test_plan) = task
        .test_plan
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("\nTest plan:\n");
        out.push_str(test_plan);
        out.push('\n');
    }
    let lane = lane_lines(&task.lane);
    if !lane.is_empty() {
        out.push_str("\nBranch/worktree hints:\n");
        out.push_str(&lane.join("\n"));
        out.push('\n');
    }
    out.push_str("\nPrior attempt:\n");
    out.push_str(&format!(
        "- attempt_id: {}\n- executor_tab_id: {}\n- status: {}\n",
        source.id,
        source.executor_tab_id,
        attempt_status(source)
    ));
    if let Some(status) = last_status {
        out.push_str(&format!("- last_report_status: {status}\n"));
    }
    if let Some(report) = last_report {
        out.push_str("\nLast report:\n");
        out.push_str(report.trim());
        out.push('\n');
    }
    out.push_str("\nWhen done, run exactly:\n");
    out.push_str(report_command);
    out.push('\n');
    super::worker_identity::attach_worker_identity(&identity, &out)
}

fn build_compete_prompt(
    plan_id: &str,
    task: &Task,
    group_id: &str,
    lane: u32,
    model_label: &str,
    report_command: &str,
    manifest: &[crate::plan::PlanSkillManifestEntry],
) -> String {
    let identity =
        super::worker_identity::compose_task_worker_identity(task, task.personality.as_deref());
    let mut out = String::new();
    out.push_str(crate::service_plan::competition::COMPETITION_WORKER_ISOLATION_PROMPT);
    out.push_str("\n\n");
    out.push_str(&format!("Plan: {plan_id}\nTask: {}\n", task.id));
    out.push_str(&super::task_brief::render_plan_skill_manifest(
        plan_id, manifest,
    ));
    out.push_str(&format!(
        "Competition group: {group_id}\nAttempt lane: {lane}\n"
    ));
    out.push_str(&format!("Model: {model_label}\n"));
    out.push_str(&format!("Task name: {}\n", task.name));
    if let Some(hint) = task
        .competition
        .as_ref()
        .and_then(|competition| competition.judge_hint.as_deref())
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("\nJudge hint:\n");
        out.push_str(hint.trim());
        out.push('\n');
    }
    if let Some(strides) = super::stride_context::render_worker_strides(&task.milestones) {
        out.push('\n');
        out.push_str(&strides);
    }
    out.push_str("\nTask body:\n");
    if task.body.trim().is_empty() {
        out.push_str("(no body recorded)\n");
    } else {
        out.push_str(task.body.trim());
        out.push('\n');
    }
    if let Some(intent) = task.intent.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("\nIntent:\n");
        out.push_str(intent.trim());
        out.push('\n');
    }
    if !task.acceptance_criteria.is_empty() {
        out.push_str("\nAcceptance criteria:\n");
        for criterion in &task.acceptance_criteria {
            out.push_str("- ");
            out.push_str(criterion);
            out.push('\n');
        }
    }
    if let Some(correctness) = task
        .correctness
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("\nCorrectness:\n");
        out.push_str(correctness);
        out.push('\n');
    }
    if let Some(test_plan) = task
        .test_plan
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("\nTest plan:\n");
        out.push_str(test_plan);
        out.push('\n');
    }
    let lane_lines = lane_lines(&task.lane);
    if !lane_lines.is_empty() {
        out.push_str("\nBase lane hints:\n");
        out.push_str(&lane_lines.join("\n"));
        out.push('\n');
    }
    out.push_str("\nWhen done, run exactly:\n");
    out.push_str(report_command);
    out.push('\n');
    super::worker_identity::attach_worker_identity(&identity, &out)
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
        id: attempt_id
            .map(str::to_string)
            .unwrap_or_else(gen_attempt_id),
        executor_tab_id: tab_id.to_string(),
        provider,
        agent_type: provider_name_from_model(task.model.as_deref()),
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

fn provider_name_from_model(value: Option<&str>) -> Option<String> {
    split_provider_model(value).0
}

pub(super) fn live_attempt_count(task: &Task) -> usize {
    task.execution_attempts
        .iter()
        .filter(|attempt| attempt.closed_at.is_none())
        .count()
}

fn find_attempt<'a>(task: &'a Task, attempt_id: &str) -> Option<&'a TaskExecutionAttempt> {
    task.execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
}

fn attempt_view(
    plan_id: &str,
    task: &Task,
    attempt: TaskExecutionAttempt,
    prompt: Option<String>,
) -> TaskAttemptView {
    let (last_report, last_report_status, last_report_at) = last_report_for_attempt(task, &attempt);
    TaskAttemptView {
        attempt_id: attempt.id.clone(),
        status: attempt_status(&attempt),
        report_command: task_report_command(plan_id, &task.id),
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
    ) -> Vec<String> {
        let workspace_id = workspace_id.trim();
        if workspace_id.is_empty() || limit == 0 {
            return Vec::new();
        }
        let target_session = session_id.map(str::trim).filter(|value| !value.is_empty());
        let mut entries: Vec<(u64, String)> = self
            .list_full()
            .into_iter()
            .filter(|plan| plan.status == crate::plan::PlanStatus::Active)
            .flat_map(|plan| plan.tasks.into_iter())
            .filter(|task| task.lane.workspace_id.as_deref() == Some(workspace_id))
            .flat_map(|task| task.execution_attempts.into_iter())
            .filter(|attempt| attempt.role.as_deref() == Some("worker"))
            .filter_map(|attempt| {
                let title = attempt.executor_title?.trim().to_string();
                if title.is_empty() {
                    return None;
                }
                if let Some(target_session) = target_session {
                    if attempt.session_id.as_deref() != Some(target_session) {
                        return None;
                    }
                }
                Some((
                    attempt.assigned_at.or(attempt.reported_at).unwrap_or(0),
                    title,
                ))
            })
            .collect();
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        entries.truncate(limit);
        entries.into_iter().map(|(_, title)| title).collect()
    }

    pub fn update_attempt_executor_identity(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        title: Option<&str>,
        session_id: Option<&str>,
        session_binding_nonce: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let title = title.map(str::trim).filter(|value| !value.is_empty());
        let session_id = session_id.map(str::trim).filter(|value| !value.is_empty());
        let session_binding_nonce = session_binding_nonce
            .map(str::trim)
            .filter(|value| !value.is_empty());
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .rev()
                .find(|attempt| attempt.executor_tab_id == tab_id)
                .ok_or(PlanError::TaskNotFound)?;
            attempt.executor_title = title.map(str::to_string);
            attempt.session_id = session_id.map(str::to_string);
            attempt.session_binding_nonce = session_binding_nonce.map(str::to_string);
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn build_task_compete_drafts(
        &self,
        plan_id: &str,
        task_id: &str,
        request: TaskCompeteRequest,
    ) -> Result<Vec<TaskCompeteDraft>, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .ok_or(PlanError::TaskNotFound)?;
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
            .unwrap_or_else(|| plan.project_path.clone());
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
                plan_id: plan.id.clone(),
                task_id: task.id.clone(),
                competition_group_id: group_id.clone(),
                lane,
                provider,
                planned_model: task.model.clone(),
                model,
                parent_tab_id: request.parent_tab_id.clone().or(plan.owner.tab_id.clone()),
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
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .ok_or(PlanError::TaskNotFound)?;
        Ok(task
            .execution_attempts
            .iter()
            .cloned()
            .map(|attempt| attempt_view(&plan.id, task, attempt, None))
            .collect())
    }

    pub fn show_task_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
    ) -> Result<TaskAttemptView, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .ok_or(PlanError::TaskNotFound)?;
        let source = find_attempt(task, attempt_id)
            .cloned()
            .ok_or(PlanError::TaskNotFound)?;
        let (task_provider, task_model) = split_provider_model(task.model.as_deref());
        let provider = source
            .provider
            .clone()
            .or(task_provider)
            .unwrap_or_else(|| "claude".into());
        let model = source.model.clone().or(task_model);
        let report_command = task_report_command(&plan.id, &task.id);
        let prompt = build_prompt(
            &plan.id,
            task,
            &source,
            &provider,
            model.as_deref(),
            &report_command,
            &source.plan_skill_manifest,
            source.authorized_gates.as_deref().unwrap_or(&[]),
        );
        Ok(attempt_view(&plan.id, task, source, Some(prompt)))
    }

    pub fn build_task_respawn_draft(
        &self,
        plan_id: &str,
        task_id: &str,
        request: TaskRespawnRequest,
    ) -> Result<TaskRespawnDraft, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .ok_or(PlanError::TaskNotFound)?;
        let source = find_attempt(task, &request.attempt_id)
            .cloned()
            .ok_or(PlanError::TaskNotFound)?;
        let (task_provider, task_model) = split_provider_model(task.model.as_deref());
        let provider = request
            .provider
            .or(source.provider.clone())
            .or(task_provider)
            .unwrap_or_else(|| "claude".into());
        let model = request.model.or(source.model.clone()).or(task_model);
        let report_command = task_report_command(plan_id, &task.id);
        let manifest = super::plan_skills::plan_skill_manifest(self, plan_id)?;
        let explicit_title = request.title.is_some();
        let cwd = source
            .worktree_path
            .clone()
            .or_else(|| task.lane.worktree_path.clone())
            .unwrap_or_else(|| plan.project_path.clone());
        let task_prompt = build_prompt(
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
            plan_id: plan.id,
            task_id: task.id.clone(),
            source_attempt: source.clone(),
            provider,
            planned_model: task.model.clone(),
            model,
            parent_tab_id: request
                .parent_tab_id
                .or(source.parent_tab_id.clone())
                .or(plan.owner.tab_id),
            workspace_id: request.workspace_id.or(task.lane.workspace_id.clone()),
            title: request.title,
            explicit_title,
            cwd,
            task_prompt,
            report_command,
            plan_skill_manifest: manifest,
            authorized_gates: source.authorized_gates.clone(),
        })
    }

    pub fn record_task_respawn_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        draft: &TaskRespawnDraft,
        role: Option<&str>,
        now_ms: u64,
    ) -> Result<TaskAttemptCreated, PlanError> {
        let mut created = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if plan.tasks[task_index].status == TaskStatus::Pending {
                super::review_gate::validate_dispatch_review_capacity(
                    plan,
                    &plan.tasks[task_index],
                )?;
            }
            let task = &mut plan.tasks[task_index];
            task.assigned_tab_id = Some(tab_id.into());
            task.lane.tab_id = Some(tab_id.into());
            task.taken_by_role = role.map(str::to_string);
            task.peer = None;
            task.status = TaskStatus::Pending;
            task.status_changed_at = now_ms;
            task.status_label = Some("starting".into());
            task.updated_at = now_ms;
            let mut attempt = new_assignment_attempt(task, tab_id, None, role, now_ms);
            attempt.provider = Some(draft.provider.clone());
            attempt.agent_type = Some(draft.provider.clone());
            attempt.model = draft.model.clone();
            attempt.executor_title = draft.title.clone();
            attempt.parent_tab_id = draft.parent_tab_id.clone();
            attempt.plan_skill_manifest = draft.plan_skill_manifest.clone();
            attempt.authorized_gates = draft.authorized_gates.clone();
            record_noncompetitive_replacement_attempt(
                &mut task.execution_attempts,
                attempt.clone(),
                now_ms,
            );
            created = Some(attempt);
            Ok(())
        })?;
        Ok(TaskAttemptCreated {
            plan_id: plan_id.into(),
            task_id: task_id.into(),
            attempt: created.expect("attempt set before successful write"),
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
        self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if plan.tasks[task_index].status == TaskStatus::Pending {
                super::review_gate::validate_dispatch_review_capacity(
                    plan,
                    &plan.tasks[task_index],
                )?;
            }
            let task = &mut plan.tasks[task_index];
            task.status_label = Some("competitive".into());
            task.updated_at = now_ms;
            let mut attempt = new_assignment_attempt(task, tab_id, None, role, now_ms);
            attempt.provider = Some(draft.provider.clone());
            attempt.agent_type = Some(draft.provider.clone());
            attempt.model = draft.model.clone();
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
        })?;
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
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
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
        })?;
        Ok(TaskAttemptCreated {
            plan_id: plan_id.into(),
            task_id: task_id.into(),
            attempt: judged.expect("attempt set before successful write"),
        })
    }

    pub fn record_task_attempt_report(
        &self,
        plan_id: &str,
        task_id: &str,
        from: &AgentRef,
        message: &str,
        status: Option<ReportStatus>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let Some(reporter_tab) = from.tab_id.as_deref() else {
            return Ok(());
        };
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            // Reports attach to the assigned executor's live attempt.
            // They never create or rebind an attempt.
            let executor_tab = task.assigned_tab_id.clone();
            let target_idx = executor_tab
                .as_deref()
                .and_then(|executor| {
                    task.execution_attempts.iter().rposition(|attempt| {
                        attempt.executor_tab_id == executor && attempt.closed_at.is_none()
                    })
                })
                .or_else(|| {
                    task.execution_attempts
                        .iter()
                        .rposition(|attempt| attempt.executor_tab_id == reporter_tab)
                });
            if let Some(attempt) = target_idx.map(|idx| &mut task.execution_attempts[idx]) {
                let reporter_tab_id =
                    (attempt.executor_tab_id != reporter_tab).then(|| reporter_tab.to_string());
                attempt.report = Some(TaskExecutionReportSnapshot {
                    id: format!("report_{now_ms}"),
                    message: Some(message.into()),
                    status: status.map(report_status_key),
                    reporter_tab_id,
                    plan_skill_manifest: attempt.plan_skill_manifest.clone(),
                    loaded_plan_skills: attempt.loaded_plan_skills.clone(),
                    ..Default::default()
                });
                attempt.reported_at = Some(now_ms);
            }
            Ok(())
        })
    }

    /// Ingest a worker report for its currently assigned task in one plan write.
    /// An accepted `in_review` report advances the task; every other
    /// accepted report remains a handoff milestone without changing task state.
    pub fn ingest_bound_worker_report(
        &self,
        plan_id: &str,
        task_id: &str,
        from: &AgentRef,
        report: BoundWorkerReport,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        let Some(reporter_tab) = from.tab_id.as_deref() else {
            return Ok(false);
        };
        let committed_review = report.status == Some(ReportStatus::InReview);
        self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if plan.tasks[task_index].assigned_tab_id.as_deref() != Some(reporter_tab) {
                return Ok(false);
            }
            super::task_attempt_gates::validate_authorized_gate_claim(
                &plan.tasks[task_index],
                reporter_tab,
                report.test_cmd.as_deref(),
            )?;
            let interruption = &plan.tasks[task_index].interruption;
            if interruption.handoff_required
                || (interruption.owner_epoch > 0
                    && interruption.owner_tab_id.as_deref() != Some(reporter_tab))
            {
                return Ok(false);
            }
            if interruption.owner_epoch > 0 {
                let authority = report
                    .authority
                    .as_ref()
                    .ok_or_else(|| PlanError::Io("report caller authority is missing".into()))?;
                super::interruption::authorize_task_mutation(
                    plan,
                    &plan.tasks[task_index],
                    authority,
                )?;
                if interruption.owner_tab_id.as_deref() == Some(&authority.tab_id) {
                    let exact_attempt =
                        plan.tasks[task_index]
                            .execution_attempts
                            .iter()
                            .any(|attempt| {
                                attempt.executor_tab_id == authority.tab_id
                                    && attempt.interruption.owner_epoch == authority.owner_epoch
                                    && attempt.closed_at.is_none()
                            });
                    if !exact_attempt {
                        return Err(PlanError::Io("report attempt authority changed".into()));
                    }
                }
            }
            if plan.tasks[task_index]
                .execution_attempts
                .iter()
                .filter(|attempt| attempt.executor_tab_id == reporter_tab)
                .filter_map(|attempt| attempt.reported_at)
                .any(|reported_at| reported_at >= now_ms)
            {
                return Ok(false);
            }
            let current = plan.tasks[task_index].status.clone();
            if matches!(current, TaskStatus::Done | TaskStatus::Cancelled) {
                return Ok(false);
            }
            if committed_review && current != TaskStatus::InReview {
                super::mutations::validate_task_status_transition(
                    plan,
                    &plan.tasks[task_index].id,
                    &current,
                    &TaskStatus::InReview,
                    None,
                )?;
            }
            let task = &mut plan.tasks[task_index];
            if task.lane.branch.as_deref().unwrap_or("").is_empty() {
                task.lane.branch = report
                    .branch
                    .clone()
                    .filter(|branch| !branch.trim().is_empty());
            }
            task.milestones.push(Milestone {
                id: crate::plan::ids::short_id("ms"),
                from: from.clone(),
                kind: "worker_report".into(),
                text: match report.status {
                    Some(status) => format!("{}: {}", status.label(), report.message),
                    None => report.message.clone(),
                },
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            let owner_epoch = task.interruption.owner_epoch;
            let report_attempt_index = task.execution_attempts.iter_mut().rposition(|attempt| {
                attempt.executor_tab_id == reporter_tab
                    && attempt.closed_at.is_none()
                    && (owner_epoch == 0 || attempt.interruption.owner_epoch == owner_epoch)
            });
            if let Some(index) = report_attempt_index {
                let attempt = &mut task.execution_attempts[index];
                attempt.report = Some(TaskExecutionReportSnapshot {
                    id: format!("report_{now_ms}"),
                    message: Some(report.message),
                    status: report.status.map(report_status_key),
                    sha: report.sha,
                    reporter_tab_id: None,
                    changed_files: report.changed_files,
                    test_cmd: report.test_cmd,
                    test_result: report.test_result,
                    skipped: report.skipped,
                    attachments: vec![],
                    plan_skill_manifest: attempt.plan_skill_manifest.clone(),
                    loaded_plan_skills: attempt.loaded_plan_skills.clone(),
                });
                attempt.reported_at = Some(now_ms);
            }
            let entering_review = committed_review && task.status != TaskStatus::InReview;
            if entering_review {
                task.status = TaskStatus::InReview;
                task.status_changed_at = now_ms;
                super::qa::normalize_task_review_fields(task);
                if let Some(index) = report_attempt_index {
                    crate::service_plan::review_gate::reset_completed_review_continuation(
                        task, index,
                    )?;
                }
                crate::service_plan::review_gate::fix_oracle_and_consume_override(task, now_ms);
            }
            task.updated_at = now_ms;
            if entering_review {
                crate::service_plan::review_gate::spend_review_credit(&mut plan.review_policy);
            }
            Ok(committed_review)
        })
    }
}
