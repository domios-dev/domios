#[cfg(test)]
use codeterm_service_core::plan::Task;
use codeterm_service_core::service::plan::task_brief::TaskBriefInput;
use codeterm_service_core::service::plan::task_queries::TaskSection;
use codeterm_service_core::service::plan::{TaskDispatchPreflightRead, WorkerTaskPreparationRead};

/// Assemble the worker brief from the task contract and its supporting context.
#[cfg(test)]
pub(in crate::server) fn build_brief(task: &Task, sections: &[TaskSection]) -> String {
    build_brief_with_personality(task, sections, task.personality.as_deref())
}

#[cfg(test)]
pub(in crate::server) fn build_brief_with_personality(
    task: &Task,
    sections: &[TaskSection],
    effective_personality: Option<&str>,
) -> String {
    codeterm_service_core::service::plan::task_brief::render_task_brief(
        task,
        sections,
        effective_personality,
        None,
        None,
        &[],
        &[],
    )
}

pub(crate) struct DispatchBriefInput<'a> {
    brief: TaskBriefInput<'a>,
    sections: &'a [TaskSection],
    personality: Option<&'a str>,
    id: &'a str,
    kind: &'a str,
}

impl<'a> From<(&'a WorkerTaskPreparationRead, &'a TaskDispatchPreflightRead)>
    for DispatchBriefInput<'a>
{
    fn from(
        (preparation, task): (&'a WorkerTaskPreparationRead, &'a TaskDispatchPreflightRead),
    ) -> Self {
        Self {
            brief: (&preparation.brief).into(),
            sections: &preparation.brief.sections,
            personality: preparation.personality.as_deref(),
            id: &task.id,
            kind: &task.kind,
        }
    }
}

#[cfg(test)]
impl<'a> From<(&'a Task, &'a [TaskSection])> for DispatchBriefInput<'a> {
    fn from((task, sections): (&'a Task, &'a [TaskSection])) -> Self {
        Self {
            brief: task.into(),
            sections,
            personality: task.personality.as_deref(),
            id: &task.id,
            kind: &task.kind,
        }
    }
}

pub(crate) fn build_brief_with_manifest<'a>(
    input: impl Into<DispatchBriefInput<'a>>,
    plan_id: &str,
    manifest: &[codeterm_service_core::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    let input = input.into();
    let mut brief = codeterm_service_core::service::plan::task_brief::render_task_brief(
        input.brief,
        input.sections,
        input.personality,
        None,
        Some(plan_id),
        manifest,
        authorized_gates,
    );
    if input.kind == "unplanned" {
        brief.push_str("\n## Worker report\nWhen ready for review, run exactly:\n");
        brief.push_str(
            &codeterm_service_core::service::plan::task_attempts::task_report_command(
                plan_id, input.id,
            ),
        );
        brief.push('\n');
    }
    brief
}

pub(super) fn build_brief_for_capabilities<'a>(
    tool_less: bool,
    input: impl Into<DispatchBriefInput<'a>>,
    plan_id: &str,
    manifest: &[codeterm_service_core::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    let input = input.into();
    if tool_less {
        codeterm_service_core::service::plan::task_brief::render_tool_less_task_brief(
            input.brief,
            input.sections,
            input.personality,
            None,
        )
    } else {
        build_brief_with_manifest(input, plan_id, manifest, authorized_gates)
    }
}

#[cfg(test)]
mod tests;
