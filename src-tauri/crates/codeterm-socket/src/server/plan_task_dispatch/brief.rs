use codeterm_service_core::plan::Task;
use codeterm_service_core::service::plan::task_queries::TaskSection;

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

pub(crate) fn build_brief_with_manifest(
    task: &Task,
    sections: &[TaskSection],
    plan_id: &str,
    manifest: &[codeterm_service_core::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    let mut brief = codeterm_service_core::service::plan::task_brief::render_task_brief(
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
        brief.push_str(
            &codeterm_service_core::service::plan::task_attempts::task_report_command(
                plan_id, &task.id,
            ),
        );
        brief.push('\n');
    }
    brief
}

pub(super) fn build_brief_for_capabilities(
    tool_less: bool,
    task: &Task,
    sections: &[TaskSection],
    plan_id: &str,
    manifest: &[codeterm_service_core::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    if tool_less {
        codeterm_service_core::service::plan::task_brief::render_tool_less_task_brief(
            task,
            sections,
            task.personality.as_deref(),
            None,
        )
    } else {
        build_brief_with_manifest(task, sections, plan_id, manifest, authorized_gates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_less_dispatch_omits_cli_lifecycle_fragments_and_preserves_task() {
        let task: Task = serde_json::from_value(serde_json::json!({"id":"task-opaque", "name":"opaque title", "type":"unplanned", "status":"pending", "body":"opaque assignment"})).unwrap();
        let command = codeterm_service_core::service::plan::task_attempts::task_report_command(
            "plan-opaque",
            &task.id,
        );
        let ordinary = build_brief_for_capabilities(false, &task, &[], "plan-opaque", &[], &[]);
        assert!(ordinary.contains(&command));
        let managed = build_brief_for_capabilities(true, &task, &[], "plan-opaque", &[], &[]);
        assert!(!managed.contains(&command));
        assert!(!managed.contains(
            codeterm_service_core::service::plan::worker_identity::WORKER_ACK_INSTRUCTION
        ));
        assert_eq!(managed.matches("opaque assignment").count(), 1);
    }
}
