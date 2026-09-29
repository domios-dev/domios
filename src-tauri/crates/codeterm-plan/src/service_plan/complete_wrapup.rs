use crate::plan::{Plan, PlanStatus};
use crate::service_plan::heartbeat::is_plan_complete;

pub fn complete_wrapup_message(plan_id: &str) -> String {
    format!(
        "{}\n\n{}",
        super::prompts_plan::PLAN_ARCHIVE_SUGGESTION.replace("<plan>", plan_id),
        super::prompts_plan::PLAN_RETROSPECTIVE_NUDGE.replace("<plan>", plan_id),
    )
}

pub fn needs_complete_wrapup(plan: &Plan) -> bool {
    plan.status == PlanStatus::Active && is_plan_complete(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{AgentRef, OrchestratorRef, PlanMode, TaskStatus};
    use crate::service_plan::prompts_plan::PLAN_RETROSPECTIVE_NUDGE;
    use crate::service_plan::{NewTask, PlanStore};

    #[test]
    fn wrapup_message_includes_archive_and_retrospective_lines() {
        let msg = complete_wrapup_message("pl_test");
        assert!(msg.contains("codeterm plan archive pl_test"));
        assert!(msg.contains("codeterm plan retrospective pl_test"));
        assert!(msg.contains("codeterm mem skill"));
    }

    #[test]
    fn retrospective_nudge_is_verbatim() {
        assert!(PLAN_RETROSPECTIVE_NUDGE.contains("Honest beats flattering"));
    }

    #[test]
    fn needs_wrapup_for_active_complete_execution_plan() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let plan = store
            .create_plan(
                "P".into(),
                dir.path().display().to_string(),
                OrchestratorRef::from_tab("o"),
                vec![],
                1,
            )
            .unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "done".into(),
                    kind: "impl".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        store.start_task_for_test(&plan.id, &task_id, 2);
        store
            .close_task(&plan.id, &task_id, None, AgentRef::from_tab("o"), 3)
            .unwrap();
        store.set_mode(&plan.id, PlanMode::Complete, 4).unwrap();
        let plan = store.get(&plan.id).unwrap();
        assert!(needs_complete_wrapup(&plan));
    }

    #[test]
    fn wrapup_gate_is_silent_for_paused_only_execution_plan() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let plan = store
            .create_plan(
                "Paused".into(),
                dir.path().display().to_string(),
                OrchestratorRef::from_tab("orch"),
                vec![],
                1,
            )
            .unwrap();
        store.set_mode(&plan.id, PlanMode::Execution, 2).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "wait".into(),
                    kind: "impl".into(),
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        store
            .with_plan(&plan.id, 4, |plan| {
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .unwrap();
                task.status = TaskStatus::Paused;
                task.pause_reason = Some("provider quota exhausted".into());
                Ok(())
            })
            .unwrap();

        let plan = store.get(&plan.id).unwrap();
        assert!(!plan
            .tasks
            .iter()
            .any(|task| task.status.counts_as_open_work()));
        assert!(
            !needs_complete_wrapup(&plan),
            "paused-only plans have no actionable work, but are not closeout-ready"
        );
    }
}
