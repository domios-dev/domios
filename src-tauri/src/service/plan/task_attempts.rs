use serde::{Deserialize, Serialize};

use crate::agent::reports::ReportStatus;
use crate::plan::{
    AgentRef, Task, TaskExecutionAttempt, TaskExecutionReportSnapshot, TaskLaneMetadata,
};

use super::{PlanError, PlanStore};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskRespawnRequest {
    pub attempt_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub parent_pane_id: Option<String>,
    pub workspace_id: Option<String>,
    pub title: Option<String>,
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
    pub model: Option<String>,
    pub parent_pane_id: Option<String>,
    pub workspace_id: Option<String>,
    pub title: String,
    pub cwd: String,
    pub task_prompt: String,
    pub report_command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskAttemptCreated {
    pub plan_id: String,
    pub task_id: String,
    pub attempt: TaskExecutionAttempt,
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
    format!("codeterm agent report --plan {plan_id} --task {task_id} --status in_review \"STATUS: in_review\\nCHANGES: ...\\nBUILD/TYPECHECK/TESTS: ...\\nFOLLOWUP: ...\"")
}

fn latest_milestone_report(task: &Task, pane_id: &str) -> Option<(String, Option<String>, u64)> {
    task.milestones
        .iter()
        .filter(|milestone| milestone.from.pane_id.as_deref() == Some(pane_id))
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
    let milestone = latest_milestone_report(task, &attempt.executor_pane_id);
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
) -> String {
    let (last_report, last_status, _) = last_report_for_attempt(task, source);
    let mut out = String::new();
    out.push_str("You are a CodeTerm Worker respawned from a prior task attempt.\n");
    out.push_str(
        "Work only on this one task. Do not reactivate or reuse the old historical attempt.\n\n",
    );
    out.push_str(&format!("Plan: {plan_id}\nTask: {}\n", task.id));
    out.push_str(&format!("Task name: {}\n", task.name));
    out.push_str(&format!("Provider/model preference: {provider}"));
    if let Some(model) = model {
        out.push_str(&format!("/{model}"));
    }
    if let Some(guidance) = task
        .role_guidance
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("\n\nRole guidance:\n");
        out.push_str(guidance.trim());
        out.push('\n');
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
    let lane = lane_lines(&task.lane);
    if !lane.is_empty() {
        out.push_str("\nBranch/worktree hints:\n");
        out.push_str(&lane.join("\n"));
        out.push('\n');
    }
    out.push_str("\nPrior attempt:\n");
    out.push_str(&format!(
        "- attempt_id: {}\n- executor_pane_id: {}\n- status: {}\n",
        source.id,
        source.executor_pane_id,
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
    out
}

pub fn new_assignment_attempt(
    task: &Task,
    pane_id: &str,
    peer: Option<&str>,
    role: Option<&str>,
    now_ms: u64,
) -> TaskExecutionAttempt {
    let (provider, model) = split_provider_model(task.model.as_deref());
    TaskExecutionAttempt {
        id: gen_attempt_id(),
        executor_pane_id: pane_id.to_string(),
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

pub fn mark_task_assignment_closed(task: &mut Task, pane: Option<&str>, now_ms: u64) {
    let pane = pane.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    });
    if let Some(pane) = pane {
        if let Some(attempt) = task
            .execution_attempts
            .iter_mut()
            .rev()
            .find(|attempt| attempt.executor_pane_id == pane && attempt.closed_at.is_none())
        {
            attempt.closed_at = Some(now_ms);
            return;
        }
    }
    if let Some(attempt) = task
        .execution_attempts
        .iter_mut()
        .rev()
        .find(|attempt| attempt.closed_at.is_none())
    {
        attempt.closed_at = Some(now_ms);
    }
}

fn provider_name_from_model(value: Option<&str>) -> Option<String> {
    split_provider_model(value).0
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
    pub fn list_task_attempts(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Vec<TaskAttemptView>, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
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
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
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
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
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
        let title = request
            .title
            .unwrap_or_else(|| format!("{} retry", task.name));
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
        );
        Ok(TaskRespawnDraft {
            plan_id: plan.id,
            task_id: task.id.clone(),
            source_attempt: source.clone(),
            provider,
            model,
            parent_pane_id: request
                .parent_pane_id
                .or(source.parent_pane_id.clone())
                .or(plan.owner.pane_id),
            workspace_id: request.workspace_id.or(task.lane.workspace_id.clone()),
            title,
            cwd,
            task_prompt,
            report_command,
        })
    }

    pub fn record_task_respawn_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        pane_id: &str,
        draft: &TaskRespawnDraft,
        role: Option<&str>,
        now_ms: u64,
    ) -> Result<TaskAttemptCreated, PlanError> {
        let mut created = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            task.assigned_pane_id = Some(pane_id.into());
            task.lane.pane_id = Some(pane_id.into());
            task.taken_by_role = role.map(str::to_string);
            task.peer = None;
            task.updated_at = now_ms;
            let mut attempt = new_assignment_attempt(task, pane_id, None, role, now_ms);
            attempt.provider = Some(draft.provider.clone());
            attempt.agent_type = Some(draft.provider.clone());
            attempt.model = draft.model.clone();
            attempt.executor_title = Some(draft.title.clone());
            attempt.parent_pane_id = draft.parent_pane_id.clone();
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

    pub fn record_task_attempt_report(
        &self,
        plan_id: &str,
        task_id: &str,
        from: &AgentRef,
        message: &str,
        status: Option<ReportStatus>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let Some(pane_id) = from.pane_id.as_deref() else {
            return Ok(());
        };
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(attempt) = task
                .execution_attempts
                .iter_mut()
                .rev()
                .find(|attempt| attempt.executor_pane_id == pane_id)
            {
                attempt.report = Some(TaskExecutionReportSnapshot {
                    id: format!("report_{now_ms}"),
                    message: Some(message.into()),
                    status: status.map(report_status_key),
                    ..Default::default()
                });
                attempt.reported_at = Some(now_ms);
            }
            Ok(())
        })
    }
}
