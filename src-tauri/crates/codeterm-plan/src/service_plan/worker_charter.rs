use super::prompts_plan;
use super::PlanStore;
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LoopCharterRead {
    pub id: String,
    pub budget: u32,
    pub count: u32,
    pub stop_when: Option<String>,
}

impl From<&crate::plan::LoopSpec> for LoopCharterRead {
    fn from(loop_spec: &crate::plan::LoopSpec) -> Self {
        Self {
            id: loop_spec.id.clone(),
            budget: loop_spec.budget,
            count: loop_spec.count,
            stop_when: loop_spec.stop_when.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct WorkerAssignmentContext {
    task_id: String,
    task_name: String,
    personality: Option<String>,
    role_guidance: Option<String>,
    strides: Option<String>,
    loop_spec: Option<LoopCharterRead>,
    live_attempt_count: usize,
    skill_count: usize,
}

fn loop_stop_when_text(loop_spec: &LoopCharterRead) -> &str {
    loop_spec
        .stop_when
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("(none recorded)")
}

pub(super) fn loop_id_line(loop_spec: &LoopCharterRead) -> String {
    format!("- loop_id: {}", loop_spec.id)
}

pub(super) fn loop_iteration_line(loop_spec: &LoopCharterRead) -> String {
    format!("- iteration: {}/{}", loop_spec.count, loop_spec.budget)
}

pub(super) fn loop_stop_when_line(loop_spec: &LoopCharterRead) -> String {
    format!("- stop_when: {}", loop_stop_when_text(loop_spec))
}

pub(super) fn competitive_attempt_count_line(live_attempt_count: usize) -> String {
    format!("- this task currently has {live_attempt_count} live attempts.")
}

fn render_loop_context(loop_spec: &LoopCharterRead) -> String {
    format!(
        "{}\n{}\n{}\n{}\n- {}\n",
        prompts_plan::WORKER_LOOP_CONTEXT_TITLE,
        loop_id_line(loop_spec),
        loop_iteration_line(loop_spec),
        loop_stop_when_line(loop_spec),
        prompts_plan::WORKER_LOOP_EVIDENCE_RULE
    )
}

fn render_competitive_context(live_attempt_count: usize) -> String {
    let mut out = format!(
        "{}\n{}\n",
        prompts_plan::WORKER_COMPETITIVE_CONTEXT_TITLE,
        competitive_attempt_count_line(live_attempt_count)
    );
    for rule in prompts_plan::WORKER_COMPETITIVE_ISOLATION_RULES {
        out.push_str("- ");
        out.push_str(rule);
        out.push('\n');
    }
    out
}

/// Compact, self-sufficient restatement of the three Worker rules. Mirrors the
/// orchestrator-heartbeat reinjection so worker role survives context drift.
pub fn render_worker_charter(
    task_id: &str,
    identity: Option<&str>,
    loop_spec: Option<&LoopCharterRead>,
    live_attempt_count: usize,
    skill_count: usize,
) -> String {
    let mut out = format!("<codeterm-worker-charter task=\"{task_id}\">\n");
    if let Some(identity) = identity {
        out.push_str(identity);
        out.push('\n');
        out.push('\n');
    }
    out.push_str(&format!(
        "You are a Worker on task {task_id}: stay on this one task, spawn no agents, and end each turn with one `domios agent report`. If the task needs splitting, report partial with the remaining slices.\n"
    ));
    if skill_count >= 1 {
        out.push_str(prompts_plan::PLAN_SKILLS_WORKER_LINE);
    } else {
        out.push_str(super::skill_registry::PLAN_SKILL_REGISTRY_POINTER);
    }
    out.push('\n');
    if let Some(loop_spec) = loop_spec {
        out.push_str(&render_loop_context(loop_spec));
    }
    if live_attempt_count > 1 {
        out.push_str(&render_competitive_context(live_attempt_count));
    }
    out.push_str(prompts_plan::TESTING_DISCIPLINE.trim());
    out.push('\n');
    out.push_str(prompts_plan::QA_PIPELINE_DISCIPLINE.trim());
    out.push_str("\n</codeterm-worker-charter>");
    out
}

pub(super) struct WorkerAssignmentRead {
    pub(super) plan_id: String,
    pub(super) state_dir: std::path::PathBuf,
    pub(super) task_id: String,
    pub(super) task_name: String,
    pub(super) personality: Option<String>,
    pub(super) role_guidance: Option<String>,
    pub(super) milestones: Vec<crate::plan::Milestone>,
    pub(super) loop_spec: Option<LoopCharterRead>,
    pub(super) live_attempt_count: usize,
}

impl PlanStore {
    /// Prepend the worker charter for a tab currently assigned to an open task.
    /// No-op (returns `text` unchanged) for tabs with no such assignment —
    /// the orchestrator-heartbeat prepend is the symmetric counterpart.
    pub fn prepend_worker_charter_for_tab(
        &self,
        tab_id: &str,
        text: &str,
    ) -> Result<String, super::PlanError> {
        prepend_assignment(self.database.worker_assignment_projection(tab_id)?, text)
    }
}

pub(super) fn prepend_assignment(
    read: Option<WorkerAssignmentRead>,
    text: &str,
) -> Result<String, super::PlanError> {
    let Some(read) = read else {
        return Ok(text.to_string());
    };
    let context = WorkerAssignmentContext {
        task_id: read.task_id.clone(),
        task_name: read.task_name.clone(),
        personality: read.personality.clone(),
        role_guidance: read.role_guidance.clone(),
        strides: super::stride_context::render_worker_strides(&read.milestones),
        loop_spec: read.loop_spec,
        live_attempt_count: read.live_attempt_count,
        skill_count: if read.plan_id.trim().is_empty() {
            0
        } else {
            super::skill_registry::count_plan_skills(&read.state_dir)
        },
    };
    let identity = super::worker_identity::compose_worker_identity(
        context.personality.as_deref(),
        context.role_guidance.as_deref(),
        Some(context.task_name.as_str()),
    );
    let mut charter = render_worker_charter(
        &context.task_id,
        identity.as_deref(),
        context.loop_spec.as_ref(),
        context.live_attempt_count,
        context.skill_count,
    );
    if let Some(strides) = context.strides.as_deref() {
        charter = format!("{charter}\n\n{strides}");
    }
    Ok(if text.trim().is_empty() {
        charter
    } else {
        format!("{charter}\n\n{text}")
    })
}

#[cfg(test)]
mod tests;
