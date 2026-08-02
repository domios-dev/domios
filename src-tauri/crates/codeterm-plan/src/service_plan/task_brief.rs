use crate::plan::Task;

use super::task_queries::TaskSection;

pub fn render_plan_skill_manifest(
    plan_id: &str,
    plan_skills: &[crate::plan::PlanSkillManifestEntry],
) -> String {
    if plan_skills.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Plan skill manifest\n");
    out.push_str(&format!("Owning plan: `{plan_id}`. After `codeterm agent ack` and before repository or tool actions, inspect this manifest and byte-completely read every applicable skill with `codeterm plan skill show --plan {plan_id} <slug>`. Choose applicability explicitly from purpose metadata; never infer it from provider, title, cwd, or global memory.\n"));
    for skill in plan_skills {
        out.push_str(&format!(
            "- `{}` | {} | {} | `{}`\n",
            skill.slug, skill.title, skill.purpose, skill.content_digest
        ));
    }
    out.push('\n');
    out
}

pub fn render_task_brief(
    task: &Task,
    sections: &[TaskSection],
    effective_personality: Option<&str>,
    orchestrator_instruction: Option<&str>,
    plan_id: Option<&str>,
    plan_skills: &[crate::plan::PlanSkillManifestEntry],
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

    if let Some(plan_id) = plan_id.filter(|_| !plan_skills.is_empty()) {
        out.push_str(&render_plan_skill_manifest(plan_id, plan_skills));
    }

    super::worker_identity::attach_worker_identity(&identity, &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatched_worker_brief_requires_applicable_plan_skills_before_actions() {
        let manifest = vec![crate::plan::PlanSkillManifestEntry {
            slug: "dispatch-contract".into(),
            title: "Dispatch Contract".into(),
            purpose: "Use for Worker launches.".into(),
            content_digest: "sha256:abc".into(),
        }];
        let rendered = render_plan_skill_manifest("pl_zzmxgb", &manifest);
        assert_eq!(rendered.matches("## Plan skill manifest").count(), 1);
        assert!(rendered.contains("After `codeterm agent ack`"));
        assert!(rendered.contains("before repository or tool actions"));
        assert!(rendered.contains(
            "codeterm plan skill show --plan pl_zzmxgb <slug>"
        ));
        assert!(rendered.contains("sha256:abc"));
        assert!(!rendered.contains("# Alpha"));
        assert!(render_plan_skill_manifest("pl_zzmxgb", &[]).is_empty());
    }
}
