use crate::plan::Task;
use crate::service::plan::task_queries::TaskSection;

/// Assemble the worker brief from the task contract and its supporting context.
#[cfg(test)]
pub(in crate::socket::server) fn build_brief(task: &Task, sections: &[TaskSection]) -> String {
    build_brief_with_personality(task, sections, task.personality.as_deref())
}

#[cfg(test)]
pub(in crate::socket::server) fn build_brief_with_personality(
    task: &Task,
    sections: &[TaskSection],
    effective_personality: Option<&str>,
) -> String {
    crate::service::plan::task_brief::render_task_brief(
        task,
        sections,
        effective_personality,
        None,
        None,
        &[],
        &[],
    )
}

pub(super) fn build_brief_with_manifest(
    task: &Task,
    sections: &[TaskSection],
    plan_id: &str,
    manifest: &[crate::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    let mut brief = crate::service::plan::task_brief::render_task_brief(
        task,
        sections,
        task.personality.as_deref(),
        None,
        Some(plan_id),
        manifest,
        authorized_gates,
    );
    if task.kind == "unplanned" {
        brief.push_str("\n## Worker report\nWhen ready for review, run exactly:\n");
        brief.push_str(&crate::service::plan::task_attempts::task_report_command(
            plan_id, &task.id,
        ));
        brief.push('\n');
    }
    brief
}
