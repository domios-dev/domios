use crate::plan::Task;

use super::stride_context::WorkerStrideRef;
use super::task_queries::TaskSection;

pub struct TaskBriefInput<'a> {
    id: &'a str,
    name: &'a str,
    parent_task_id: Option<&'a str>,
    body: &'a str,
    intent: Option<&'a str>,
    acceptance_criteria: &'a [String],
    correctness: Option<&'a str>,
    test_plan: Option<&'a str>,
    role_guidance: Option<&'a str>,
    executor_role: Option<&'a str>,
    strides: Vec<WorkerStrideRef<'a>>,
}

impl<'a> From<&'a Task> for TaskBriefInput<'a> {
    fn from(task: &'a Task) -> Self {
        Self {
            id: &task.id,
            name: &task.name,
            parent_task_id: task.parent_task_id.as_deref(),
            body: &task.body,
            intent: task.intent.as_deref(),
            acceptance_criteria: &task.acceptance_criteria,
            correctness: task.correctness.as_deref(),
            test_plan: task.test_plan.as_deref(),
            role_guidance: task.role_guidance.as_deref(),
            executor_role: task.executor_role.as_deref(),
            strides: task.milestones.iter().map(WorkerStrideRef::from).collect(),
        }
    }
}

impl<'a> From<&'a super::WorkerTaskBriefRead> for TaskBriefInput<'a> {
    fn from(task: &'a super::WorkerTaskBriefRead) -> Self {
        Self {
            id: &task.id,
            name: &task.name,
            parent_task_id: task.parent_task_id.as_deref(),
            body: &task.body,
            intent: task.intent.as_deref(),
            acceptance_criteria: &task.acceptance_criteria,
            correctness: task.correctness.as_deref(),
            test_plan: task.test_plan.as_deref(),
            role_guidance: task.role_guidance.as_deref(),
            executor_role: task.executor_role.as_deref(),
            strides: task
                .strides
                .iter()
                .map(|stride| WorkerStrideRef {
                    id: stride.id.as_str(),
                    kind: stride.kind.as_str(),
                    text: stride.text.as_str(),
                    ts: stride.ts,
                })
                .collect(),
        }
    }
}

pub fn render_plan_skill_manifest(
    plan_id: &str,
    plan_skills: &[crate::plan::PlanSkillManifestEntry],
) -> String {
    if plan_skills.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Plan skill manifest\n");
    out.push_str(&format!("Owning plan: `{plan_id}`. After the dispatch-specific Worker ACK and before repository or tool actions, inspect this manifest and byte-completely read every applicable skill with `domios plan skill show --plan {plan_id} <slug>`. Choose applicability explicitly from purpose metadata; never infer it from provider, title, cwd, or global memory.\n"));
    for skill in plan_skills {
        out.push_str(&format!(
            "- `{}` | {} | {} | `{}`\n",
            skill.slug, skill.title, skill.purpose, skill.content_digest
        ));
    }
    out.push('\n');
    out
}

pub fn render_authorized_gates(authorized_gates: &[String]) -> String {
    if authorized_gates.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Authorized gates\n");
    for gate in authorized_gates {
        out.push_str("- ");
        out.push_str(gate);
        out.push('\n');
    }
    out.push('\n');
    out
}

pub fn render_task_brief<'a>(
    task: impl Into<TaskBriefInput<'a>>,
    sections: &[TaskSection],
    effective_personality: Option<&str>,
    orchestrator_instruction: Option<&str>,
    plan_id: Option<&str>,
    plan_skills: &[crate::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    let task = task.into();
    let identity = super::worker_identity::compose_worker_identity_contract(
        effective_personality,
        task.role_guidance,
        task.executor_role,
        task.name,
    );
    let assignment = render_task_assignment(
        &task,
        sections,
        orchestrator_instruction,
        plan_id,
        plan_skills,
        authorized_gates,
        true,
    );
    super::worker_identity::attach_worker_identity(&identity, &assignment)
}

pub fn render_tool_less_task_brief<'a>(
    task: impl Into<TaskBriefInput<'a>>,
    sections: &[TaskSection],
    effective_personality: Option<&str>,
    orchestrator_instruction: Option<&str>,
) -> String {
    let task = task.into();
    let identity = super::worker_identity::compose_worker_identity_contract(
        effective_personality,
        task.role_guidance,
        task.executor_role,
        task.name,
    );
    let assignment = render_task_assignment(
        &task,
        sections,
        orchestrator_instruction,
        None,
        &[],
        &[],
        false,
    );
    [identity, assignment].join("\n\n")
}

fn render_task_assignment(
    task: &TaskBriefInput<'_>,
    sections: &[TaskSection],
    orchestrator_instruction: Option<&str>,
    plan_id: Option<&str>,
    plan_skills: &[crate::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
    include_read_command: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", task.name));

    if let Some(parent_task_id) = &task.parent_task_id {
        out.push_str(&format!("Parent task: `{parent_task_id}`\n\n"));
    }

    let body = task.body.trim();
    if !body.is_empty() {
        out.push_str(body);
        out.push_str("\n\n");
    }

    if let Some(intent) = task.intent.map(str::trim).filter(|value| !value.is_empty()) {
        out.push_str(&format!("## Intent\n{intent}\n\n"));
    }

    if !task.acceptance_criteria.is_empty() {
        out.push_str("## Acceptance criteria\n");
        for criterion in task.acceptance_criteria {
            out.push_str(&format!("- {criterion}\n"));
        }
        out.push('\n');
    }

    if let Some(correctness) = task
        .correctness
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str(&format!("## Correctness\n{correctness}\n\n"));
    }

    if let Some(test_plan) = task
        .test_plan
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str(&format!("## Test plan\n{test_plan}\n\n"));
    }

    out.push_str(&render_authorized_gates(authorized_gates));

    if !sections.is_empty() {
        out.push_str("## Sections\n");
        for section in sections {
            out.push_str(&format!("- {} ({})\n", section.title, section.id));
        }
        out.push('\n');
    }

    if let Some(strides) = super::stride_context::render_worker_strides_from_refs(
        &task.strides,
        plan_id,
        Some(task.id),
        include_read_command,
    ) {
        out.push_str(&strides);
        out.push('\n');
    }

    if let Some(instruction) = orchestrator_instruction
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("## Orchestrator instruction\n");
        out.push_str(instruction);
        out.push_str("\n\n");
    }

    if let Some(plan_id) = plan_id.filter(|_| !plan_skills.is_empty()) {
        out.push_str(&render_plan_skill_manifest(plan_id, plan_skills));
    }

    out
}

#[cfg(test)]
mod tests;
