use crate::plan::{Plan, TaskStatus};
use crate::service::hierarchy::heartbeat_gate::DeliveryTick;
use crate::service::plan::reminders::{orchestrator_nudge_text, worker_nudge_text};

use super::{timed_plan_reminder_targets, PlanReminderTarget};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectedTick {
    pub at_ms: u64,
    pub outcome: String,
    pub spinal_verdict: String,
    pub spinal_reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectedHeartbeat {
    pub id: String,
    pub tab_id: String,
    pub plan_id: String,
    pub text: String,
    pub cadence_ms: u64,
    pub next_due_ms: u64,
    pub reason: String,
    pub ticks: Vec<ProjectedTick>,
}

pub(crate) fn heartbeat_reason(plan_id: &str, task_ids: &[String]) -> String {
    let tasks = if task_ids.is_empty() {
        "none".to_string()
    } else {
        task_ids.join(", ")
    };
    format!("sent because tasks {tasks} are still in progress in plan {plan_id}")
}

/// Read-only view of the reminder targets the scheduler already owns.
/// `deadline_secs` is that scheduler's stored deadline for the tab.
pub(crate) fn project_for_tab(
    plans: &[Plan],
    tab_id: &str,
    interval_secs: u64,
    now_secs: u64,
    deadline_secs: Option<u64>,
    ticks: &[DeliveryTick],
    reminders_enabled: bool,
) -> Vec<ProjectedHeartbeat> {
    if !reminders_enabled || interval_secs == 0 || tab_id.is_empty() {
        return Vec::new();
    }
    let cadence_ms = interval_secs.saturating_mul(1000);
    let next_due_ms = deadline_secs
        .unwrap_or(now_secs)
        .saturating_add(interval_secs)
        .saturating_mul(1000);
    timed_plan_reminder_targets(plans)
        .into_iter()
        .filter(|target| target.tab_id == tab_id)
        .filter_map(|target| {
            project_target(
                plans,
                &target.target,
                tab_id,
                cadence_ms,
                next_due_ms,
                ticks,
            )
        })
        .collect()
}

fn project_target(
    plans: &[Plan],
    target: &PlanReminderTarget,
    tab_id: &str,
    cadence_ms: u64,
    next_due_ms: u64,
    ticks: &[DeliveryTick],
) -> Option<ProjectedHeartbeat> {
    match target {
        PlanReminderTarget::Owner { plan_id } => {
            let plan = plans.iter().find(|plan| plan.id == *plan_id)?;
            let task_ids = in_progress_ids(plan, None);
            Some(ProjectedHeartbeat {
                id: format!("system:heartbeat:{plan_id}:{tab_id}"),
                tab_id: tab_id.to_string(),
                plan_id: plan_id.clone(),
                text: orchestrator_nudge_text(plan),
                cadence_ms,
                next_due_ms,
                reason: heartbeat_reason(plan_id, &task_ids),
                ticks: view_ticks(ticks, tab_id, Some(plan_id)),
            })
        }
        PlanReminderTarget::Worker => {
            let plan = plans.iter().find(|plan| {
                plan.tasks.iter().any(|task| {
                    task.assigned_tab_id.as_deref() == Some(tab_id)
                        && task.status == TaskStatus::InProgress
                })
            })?;
            let task_ids = in_progress_ids(plan, Some(tab_id));
            Some(ProjectedHeartbeat {
                id: format!("system:heartbeat:worker:{tab_id}"),
                tab_id: tab_id.to_string(),
                plan_id: plan.id.clone(),
                text: worker_nudge_text(),
                cadence_ms,
                next_due_ms,
                reason: heartbeat_reason(&plan.id, &task_ids),
                ticks: view_ticks(ticks, tab_id, None),
            })
        }
    }
}

fn in_progress_ids(plan: &Plan, tab_id: Option<&str>) -> Vec<String> {
    plan.tasks
        .iter()
        .filter(|task| task.status == TaskStatus::InProgress)
        .filter(|task| {
            tab_id
                .map(|tab_id| task.assigned_tab_id.as_deref() == Some(tab_id))
                .unwrap_or(true)
        })
        .map(|task| task.id.clone())
        .collect()
}

fn view_ticks(ticks: &[DeliveryTick], tab_id: &str, plan_id: Option<&str>) -> Vec<ProjectedTick> {
    ticks
        .iter()
        .filter(|tick| tick.tab_id == tab_id)
        .filter(|tick| match plan_id {
            Some(plan_id) => tick.plan_id == plan_id,
            None => tick.plan_id.is_empty(),
        })
        .map(|tick| ProjectedTick {
            at_ms: tick.at_ms,
            outcome: tick.outcome.as_str().into(),
            spinal_verdict: tick.spinal_verdict.as_str().into(),
            spinal_reason: tick.spinal_reason.clone(),
        })
        .collect()
}
