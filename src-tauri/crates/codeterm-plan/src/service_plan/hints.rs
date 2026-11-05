use crate::plan::{Plan, TaskStatus};
use std::sync::atomic::{AtomicU64, Ordering};

const ORCHESTRATION_REMINDER: &str =
    "hint: orchestrating? read `domios docs ORCHESTRATION`; before dispatch, run `domios plan doctor <plan> --kind quality`";
const ORCHESTRATION_REMINDER_EVERY: u64 = 5;
static ORCHESTRATION_REMINDER_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn task_sections_hint(plan_id: &str, task_id: &str) -> String {
    format!(
        "hint: task is not linked to a plan section. If it belongs to a plan, connect it: domios plan task link {plan_id} {task_id} --sections <id-or-handle> (ids: domios plan get {plan_id} --sections)"
    )
}

pub fn plan_sections_hint(plan_id: &str) -> String {
    format!("hint: plan has no sections. Add one: domios plan section add {plan_id} --title <title> --body <summary>")
}

pub fn task_model_hint(plan_id: &str, task_id: &str) -> String {
    format!(
        "hint: task has no model/why. Capture routing intent: domios plan task update {plan_id} {task_id} --model <provider/model> --why <reason>"
    )
}

pub fn task_quality_guard_hint(
    plan_id: &str,
    task_id: &str,
    has_criteria: bool,
    has_test_plan: bool,
    has_correctness: bool,
) -> Option<String> {
    let mut missing = vec![];
    if !has_criteria {
        missing.push("acceptance criteria");
    }
    if !has_test_plan {
        missing.push("test plan");
    }
    if !has_correctness {
        missing.push("correctness vector");
    }
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "WARNING: task is missing {}. Add checkable work definition now: domios plan task add {plan_id} --name <name> --executor-role <role> --personality <name> --role-guidance <conditioning> --criterion <condition> --test-plan <how verified> --correctness <one-line vector>; existing task: {task_id}",
        missing.join(", ")
    ))
}

pub fn task_create_hints(
    plan_id: &str,
    task_id: &str,
    has_sections: bool,
    has_model: bool,
    has_why: bool,
    has_criteria: bool,
    has_test_plan: bool,
    has_correctness: bool,
) -> Vec<String> {
    let mut hints = vec![];
    if !has_sections {
        hints.push(task_sections_hint(plan_id, task_id));
    }
    if !has_model || !has_why {
        hints.push(task_model_hint(plan_id, task_id));
    }
    if let Some(hint) = task_quality_guard_hint(
        plan_id,
        task_id,
        has_criteria,
        has_test_plan,
        has_correctness,
    ) {
        hints.push(hint);
    }
    hints
}

pub fn orchestration_reminder_for_count(count: u64) -> Option<String> {
    if count == 1 || count % ORCHESTRATION_REMINDER_EVERY == 0 {
        Some(ORCHESTRATION_REMINDER.to_string())
    } else {
        None
    }
}

#[cfg(test)]
thread_local! {static FIXED_REMINDER_COUNT:std::cell::Cell<Option<u64>>=const {std::cell::Cell::new(None)};}
#[cfg(test)]
pub(in crate::service_plan) struct ReminderCountGuard(Option<u64>);
#[cfg(test)]
impl Drop for ReminderCountGuard {
    fn drop(&mut self) {
        FIXED_REMINDER_COUNT.with(|count| count.set(self.0));
    }
}
#[cfg(test)]
pub(in crate::service_plan) fn fixed_reminder_count(count: u64) -> ReminderCountGuard {
    ReminderCountGuard(FIXED_REMINDER_COUNT.with(|value| value.replace(Some(count))))
}

pub fn maybe_orchestration_reminder() -> Option<String> {
    #[cfg(test)]
    if let Some(count) = FIXED_REMINDER_COUNT.with(|value| value.get()) {
        return orchestration_reminder_for_count(count);
    }
    let count = ORCHESTRATION_REMINDER_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    orchestration_reminder_for_count(count)
}

#[derive(serde::Deserialize)]
pub struct PlannerNudgeTask {
    pub name: String,
    pub handle: Option<String>,
    pub status: TaskStatus,
    pub assigned_tab_id: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct PlannerNudgeInput {
    pub id: String,
    pub tasks: Vec<PlannerNudgeTask>,
}

impl From<&Plan> for PlannerNudgeInput {
    fn from(plan: &Plan) -> Self {
        Self {
            id: plan.id.clone(),
            tasks: plan
                .tasks
                .iter()
                .filter(|task| task.kind == "planning")
                .map(|task| PlannerNudgeTask {
                    name: task.name.clone(),
                    handle: task.handle.clone(),
                    status: task.status.clone(),
                    assigned_tab_id: task.assigned_tab_id.clone(),
                })
                .collect(),
        }
    }
}

pub fn planner_ownership_nudge(
    input: impl Into<PlannerNudgeInput>,
    caller_tab_id: Option<&str>,
) -> Option<String> {
    let plan = input.into();

    let task = plan.tasks.iter().find(|t| {
        !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled)
            && t.assigned_tab_id
                .as_deref()
                .is_some_and(|tab| caller_tab_id.map_or(true, |caller| tab != caller))
    })?;
    let handle = task
        .handle
        .as_deref()
        .filter(|h| !h.is_empty())
        .unwrap_or(&task.name);
    Some(format!(
        "hint: plan {} has an active Planner lane (task {}); route planning input there or record a stride.",
        plan.id, handle
    ))
}

#[cfg(test)]
mod tests;
