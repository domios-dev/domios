use crate::plan::{Plan, PlanStatus};
use crate::service::plan::heartbeat::is_plan_complete;

pub fn complete_wrapup_message(plan_id: &str) -> String {
    format!(
        "{}\n\n{}",
        crate::prompts::PLAN_ARCHIVE_SUGGESTION.replace("<plan>", plan_id),
        crate::prompts::PLAN_RETROSPECTIVE_NUDGE.replace("<plan>", plan_id),
    )
}

pub fn needs_complete_wrapup(plan: &Plan) -> bool {
    plan.status == PlanStatus::Active && is_plan_complete(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan, PlanMode};
    use crate::prompts::PLAN_RETROSPECTIVE_NUDGE;

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
        let mut plan = Plan::new("P".into(), "/tmp".into(), OrchestratorRef::from_pane("o"));
        plan.mode = PlanMode::Complete;
        assert!(needs_complete_wrapup(&plan));
    }
}
