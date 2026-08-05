use crate::plan::Task;
use crate::service::plan::task_queries::TaskSection;

/// Assemble the worker brief: body, acceptance, test plan, section links,
/// strides (milestones — T4 enriches later), and the mandatory report command.
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
    )
}

pub(super) fn build_brief_with_manifest(
    task: &Task,
    sections: &[TaskSection],
    plan_id: &str,
    manifest: &[crate::plan::PlanSkillManifestEntry],
) -> String {
    crate::service::plan::task_brief::render_task_brief(
        task,
        sections,
        task.personality.as_deref(),
        None,
        Some(plan_id),
        manifest,
    )
}
