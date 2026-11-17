use codeterm_plan::service_plan::{ReminderCandidateRead, ReminderTargetRead};

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, PartialEq, Eq)]
pub enum PlanReminderTarget {
    Owner { plan_id: String },
    Worker,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TimedPlanReminderTarget {
    pub tab_id: String,
    pub target: PlanReminderTarget,
}

pub fn effective_interval(
    plans: &[ReminderCandidateRead],
    target: &PlanReminderTarget,
    machine_default: u64,
) -> u64 {
    match target {
        PlanReminderTarget::Owner { plan_id } => plans
            .iter()
            .find(|plan| &plan.id == plan_id)
            .map_or(machine_default, |plan| {
                plan.control_loop.interval(machine_default)
            }),
        PlanReminderTarget::Worker => machine_default,
    }
}

pub fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn timed_plan_reminder_targets(
    plans: &[ReminderCandidateRead],
) -> Vec<TimedPlanReminderTarget> {
    codeterm_plan::service_plan::reminder_targets(plans)
        .into_iter()
        .map(|target| match target {
            ReminderTargetRead::Owner { tab_id, plan_id } => TimedPlanReminderTarget {
                tab_id,
                target: PlanReminderTarget::Owner { plan_id },
            },
            ReminderTargetRead::Worker { tab_id } => TimedPlanReminderTarget {
                tab_id,
                target: PlanReminderTarget::Worker,
            },
        })
        .collect()
}
