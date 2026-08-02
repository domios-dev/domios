use crate::plan::Task;

use super::task_queries::TaskSection;

pub fn render_task_brief(
    task: &Task,
    sections: &[TaskSection],
    effective_personality: Option<&str>,
    orchestrator_instruction: Option<&str>,
) -> String {
    let mut out = String::new();
    let identity =
        super::worker_identity::compose_task_worker_identity(task, effective_personality);
    out.push_str(&format!("# {}\n\n", task.name));

    let body = task.body.trim();
    if !body.is_empty() {
        out.push_str(body);
        out.push_str("\n\n");
    }

    if let Some(intent) = task
        .intent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str(&format!("## Intent\n{intent}\n\n"));
    }

    if !task.acceptance_criteria.is_empty() {
        out.push_str("## Acceptance criteria\n");
        for criterion in &task.acceptance_criteria {
            out.push_str(&format!("- {criterion}\n"));
        }
        out.push('\n');
    }

    if let Some(correctness) = task
        .correctness
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str(&format!("## Correctness\n{correctness}\n\n"));
    }

    if let Some(test_plan) = task
        .test_plan
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str(&format!("## Test plan\n{test_plan}\n\n"));
    }

    if !sections.is_empty() {
        out.push_str("## Sections\n");
        for section in sections {
            out.push_str(&format!("- {} ({})\n", section.title, section.id));
        }
        out.push('\n');
    }

    if !task.milestones.is_empty() {
        out.push_str("## Strides\n");
        for milestone in &task.milestones {
            out.push_str(&format!("- [{}] {}\n", milestone.kind, milestone.text));
        }
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

    super::worker_identity::attach_worker_identity(&identity, &out)
}
