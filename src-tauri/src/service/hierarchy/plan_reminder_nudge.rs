use crate::plan::{Plan, PlanMode, PlanStatus, TaskStatus};
use crate::service::plan::heartbeat::is_plan_complete;
use crate::service::{ServiceContext, ServiceState};
use codeterm_plan::service_plan::state::{
    plan_needs_orchestrator_action, task_projection, DependencyWait, TaskPresentation,
};
use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_PLAN_REMINDER_POLL_SECS: u64 = 60;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PlanReminderTarget {
    Owner { plan_id: String },
    Worker,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TimedPlanReminderTarget {
    pub(crate) tab_id: String,
    pub(crate) target: PlanReminderTarget,
}

pub(crate) fn start_plan_reminder_scheduler(svc: ServiceState) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(plan_reminder_poll_secs())).await;
            let svc_for_scan = svc.clone();
            if let Err(e) = tauri::async_runtime::spawn_blocking(move || {
                svc_for_scan.send_due_plan_time_reminders();
                svc_for_scan.run_lane_watchdog();
            })
            .await
            {
                log::warn!("[plan-reminder] scheduler task failed: {}", e);
            }
        }
    });
}

fn plan_reminder_poll_secs() -> u64 {
    let cfg = crate::config::load_config().orchestration;
    cfg.nudge_interval_secs
        .clamp(1, MAX_PLAN_REMINDER_POLL_SECS)
}

fn send_due_plan_time_reminders_with(
    plans: &[Plan],
    counter: &crate::service::plan::reminders::TurnCounter,
    now: u64,
    interval_secs: u64,
    deliver: impl Fn(
        &str,
        &PlanReminderTarget,
        crate::service::plan::reminders::NudgeKind,
    ) -> Result<(), String>,
) {
    let targets = timed_plan_reminder_targets(plans);
    let eligible_tabs: HashSet<String> =
        targets.iter().map(|target| target.tab_id.clone()).collect();
    counter.retain_time_reminders(&eligible_tabs);
    let candidates = targets.len();
    let mut due = 0usize;
    for timed in targets {
        counter.record_time_tick(&timed.tab_id, now);
        let is_due = counter.time_nudge_due(&timed.tab_id, now, interval_secs);
        if !is_due {
            continue;
        }
        due += 1;
        counter.record_time_due(&timed.tab_id, now);
        match deliver(&timed.tab_id, &timed.target, timed.target.time_nudge_kind()) {
            Ok(()) => {
                log::info!(
                    "[plan-reminder] delivered {:?} nudge to Tab {}",
                    timed.target.time_nudge_kind(),
                    timed.tab_id
                );
                counter.mark_time_nudged(&timed.tab_id, now);
                counter.record_time_delivery(&timed.tab_id, now);
            }
            Err(reason) => {
                log::warn!(
                    "[plan-reminder] FAILED to deliver nudge to Tab {} (target {:?}): {}",
                    timed.tab_id,
                    timed.target,
                    reason
                );
                counter.record_time_failure(&timed.tab_id, reason);
            }
        }
    }
    log::info!("[plan-reminder] tick: {candidates} candidate Tab(s), {due} due");
}

impl ServiceContext {
    fn send_due_plan_time_reminders(&self) {
        let cfg = crate::config::load_config().orchestration;
        if !cfg.plan_reminders || cfg.nudge_interval_secs == 0 {
            self.plan_turn_counter.clear_time_reminders();
            return;
        }
        let now = current_epoch_secs();
        send_due_plan_time_reminders_with(
            &self.plan_store.list_full(),
            &self.plan_turn_counter,
            now,
            cfg.nudge_interval_secs,
            |tab_id, target, kind| self.deliver_plan_reminder_target(tab_id, target, kind),
        );
        self.send_due_complete_wrapup_reminders(now, cfg.nudge_interval_secs);
    }

    /// Lifecycle mutations use this to clear only deadlines that became
    /// ineligible (paused, complete, parked/closed with no open work, or
    /// archived). Active execution owners keep their original deadline.
    pub(crate) fn reconcile_time_reminder_eligibility(&self) {
        let targets = timed_plan_reminder_targets(&self.plan_store.list_full());
        self.retain_time_reminder_eligibility(&targets);
    }

    fn retain_time_reminder_eligibility(&self, targets: &[TimedPlanReminderTarget]) {
        let eligible_tabs = targets.iter().map(|target| target.tab_id.clone()).collect();
        self.plan_turn_counter.retain_time_reminders(&eligible_tabs);
    }

    fn send_due_complete_wrapup_reminders(&self, now: u64, interval_secs: u64) {
        for (tab_id, plan_id) in complete_wrapup_targets(&self.plan_store.list_full()) {
            if !self.plan_turn_counter.complete_wrapup_due(&plan_id) {
                continue;
            }
            if !self
                .plan_turn_counter
                .time_nudge_due(&tab_id, now, interval_secs)
            {
                continue;
            }
            if let Err(reason) = self.ensure_plan_reminder_routable(&tab_id) {
                log::warn!("[plan-reminder] complete wrapup not delivered to {tab_id}: {reason}");
                continue;
            }
            let text = crate::service::plan::complete_wrapup::complete_wrapup_message(&plan_id);
            if self.deliver_automated_plan_reminder(&tab_id, &text).is_ok() {
                self.plan_turn_counter.mark_complete_wrapup_sent(&plan_id);
                self.plan_turn_counter.mark_time_nudged(&tab_id, now);
            }
        }
    }

    fn deliver_plan_reminder_target(
        &self,
        tab_id: &str,
        target: &PlanReminderTarget,
        kind: crate::service::plan::reminders::NudgeKind,
    ) -> Result<(), String> {
        self.ensure_plan_reminder_routable(tab_id)?;
        let state_key = match target {
            PlanReminderTarget::Owner { plan_id } => self
                .plan_store
                .get(plan_id)
                .map(|plan| plan_reminder_state_key(plan_id, kind, &plan))
                .unwrap_or_else(|| format!("owner:{plan_id}:{kind:?}")),
            PlanReminderTarget::Worker => format!("worker:{tab_id}:{kind:?}"),
        };
        let now = current_epoch_secs();
        if !self
            .plan_turn_counter
            .reminder_content_due(tab_id, &state_key, now)
        {
            return Ok(());
        }
        let mut text = match target {
            PlanReminderTarget::Owner { plan_id } => self
                .plan_store
                .get(plan_id)
                .map(|plan| crate::service::plan::reminders::orchestrator_nudge_text(&plan))
                .ok_or_else(|| format!("plan {plan_id} not found; retry on next scheduler tick"))?,
            PlanReminderTarget::Worker => crate::service::plan::reminders::worker_nudge_text(),
        };
        let delivered = match target {
            PlanReminderTarget::Owner { plan_id } => {
                if let Some(named) = self.aged_task_callout(plan_id) {
                    text.push_str("\n\n");
                    text.push_str(&named);
                }
                let resolution = self
                    .plan_store
                    .resolve_planner_tab(plan_id, |pid| self.tab_probe(pid));
                if let Some(prefix) =
                    crate::service::plan::peer_mandate::planner_pairing_nudge_prefix(&resolution)
                {
                    text = format!("{prefix}\n\n{text}");
                }
                // Planner by name (state + binding derived, not tab-guessed).
                if let Some(planning_state) = self.plan_store.get(plan_id).map(|p| p.planning_state)
                {
                    if let Some(defer) =
                        crate::service::plan::peer_mandate::planning_structure_nudge(
                            planning_state,
                            &resolution,
                        )
                    {
                        text = format!("{defer}\n\n{text}");
                    }
                }
                crate::service::orchestrator_digests::deliver_orchestrator_notice_exact(
                    self, plan_id, tab_id, "system", &text,
                )
                .map(|_| {
                    self.record_kpi_event(
                        tab_id,
                        crate::agent::kpi_ledger::KpiEventKind::Nudge,
                        text.len(),
                    );
                })
            }
            PlanReminderTarget::Worker => {
                let text = self
                    .plan_store
                    .prepend_worker_charter_for_tab(tab_id, &text);
                self.deliver_automated_plan_reminder(tab_id, &text)
            }
        };
        delivered.map(|()| {
            self.plan_turn_counter
                .mark_reminder_content_delivered(tab_id, state_key, now);
        })
    }

    fn ensure_plan_reminder_routable(&self, tab_id: &str) -> Result<(), String> {
        let live = {
            let mux = self.mux.read();
            mux.resolve_tab_id(tab_id)
                .and_then(|resolved| mux.get_tab(&resolved))
                .is_some_and(crate::service::plan_reconcile::tab_has_live_agent)
        };
        if live {
            Ok(())
        } else {
            Err("agent_unavailable: plan reminder target has no verified live agent session".into())
        }
    }

    fn deliver_automated_plan_reminder(&self, tab_id: &str, text: &str) -> Result<(), String> {
        let envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder("system");
        crate::service::messenger::Messenger::deliver(self, tab_id, envelope, text)
            .map(|_| {
                self.record_kpi_event(
                    tab_id,
                    crate::agent::kpi_ledger::KpiEventKind::Nudge,
                    text.len(),
                );
            })
            .map_err(|e| {
                log::warn!("[plan-reminder] deliver to {} failed: {}", tab_id, e);
                format!("{e}; retry on next scheduler tick")
            })
    }

    // at concrete drift instead of a generic reminder. None when nothing is aged.
    fn aged_task_callout(&self, plan_id: &str) -> Option<String> {
        let plan = self.plan_store.get(plan_id)?;
        let now = current_epoch_secs().saturating_mul(1000);
        aged_task_callout_text(&plan, now)
    }
}

fn plan_reminder_state_key(
    plan_id: &str,
    kind: crate::service::plan::reminders::NudgeKind,
    plan: &Plan,
) -> String {
    let tasks = plan
        .tasks
        .iter()
        .map(|task| {
            let projection = task_projection(plan, task);
            let waits = projection
                .dependency_waits
                .iter()
                .map(dependency_wait_key)
                .collect::<Vec<_>>()
                .join("|");
            format!(
                "{}:{:?}:{:?}:{waits}",
                task.id, projection.presentation, task.status_label
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "owner:{plan_id}:{kind:?}:{:?}:{:?}:{tasks}",
        plan.status, plan.mode
    )
}

fn dependency_wait_key(wait: &DependencyWait) -> String {
    match wait.status.as_ref() {
        Some(status) => format!("{}:{}", wait.task_id, task_status_text(status)),
        None => format!("{}:missing", wait.task_id),
    }
}

impl PlanReminderTarget {
    fn time_nudge_kind(&self) -> crate::service::plan::reminders::NudgeKind {
        match self {
            Self::Owner { .. } => crate::service::plan::reminders::NudgeKind::Orchestrator,
            Self::Worker => crate::service::plan::reminders::NudgeKind::Worker,
        }
    }
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn timed_plan_reminder_targets(plans: &[Plan]) -> Vec<TimedPlanReminderTarget> {
    let mut targets = BTreeMap::<String, PlanReminderTarget>::new();
    for plan in plans.iter().filter(|plan| {
        plan.status == PlanStatus::Active
            && plan.mode == PlanMode::Execution
            && !is_plan_complete(plan)
            && plan_needs_orchestrator_action(plan)
    }) {
        if let Some(tab_id) = non_empty(plan.owner.tab_id.as_deref()) {
            targets.insert(
                tab_id.to_string(),
                PlanReminderTarget::Owner {
                    plan_id: plan.id.clone(),
                },
            );
        }
    }
    for plan in plans.iter().filter(|plan| {
        plan.status == PlanStatus::Active
            && plan.mode == PlanMode::Execution
            && !is_plan_complete(plan)
    }) {
        for task in plan.tasks.iter().filter(|task| {
            matches!(
                task.status,
                TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Blocked
            ) && task_projection(plan, task).presentation != TaskPresentation::WaitingOnDependencies
        }) {
            let Some(tab_id) = non_empty(task.assigned_tab_id.as_deref()) else {
                continue;
            };
            targets
                .entry(tab_id.to_string())
                .or_insert(PlanReminderTarget::Worker);
        }
    }
    targets
        .into_iter()
        .map(|(tab_id, target)| TimedPlanReminderTarget { tab_id, target })
        .collect()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    (!value.is_empty()).then_some(value)
}

fn aged_task_callout_text(plan: &Plan, now_ms: u64) -> Option<String> {
    let aged = crate::plan::reconcile::aged_open_tasks(plan, now_ms);
    if aged.is_empty() {
        return None;
    }
    let listed = aged
        .iter()
        .take(5)
        .map(|item| {
            let task = plan.tasks.iter().find(|task| task.id == item.task_id);
            format!(
                "- {} \"{}\" ({}, aged {}h)",
                item.task_id,
                item.name,
                aged_task_status_text(item, task),
                item.age_ms / 3_600_000
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!("Stale tasks recorded as drift:\n{listed}"))
}

fn aged_task_status_text(
    item: &crate::plan::reconcile::AgedOpenTask,
    task: Option<&crate::plan::Task>,
) -> String {
    match item.presentation {
        TaskPresentation::HandoffRequired => "handoff required".into(),
        TaskPresentation::ActionablePending => "actionable pending".into(),
        TaskPresentation::WaitingOnDependencies => {
            let waits = item
                .dependency_waits
                .iter()
                .map(|wait| match wait.status.as_ref() {
                    Some(status) => {
                        format!("{} ({})", wait.task_id, task_status_text(status))
                    }
                    None => format!("{} (missing)", wait.task_id),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("waiting on dependencies: {waits}")
        }
        TaskPresentation::InProgress => "in progress".into(),
        TaskPresentation::InReview => "in review".into(),
        TaskPresentation::QueuedForQa => {
            match task.and_then(|task| non_empty(task.status_label.as_deref())) {
                Some(reason) => format!("queued for QA: {reason}"),
                None => "queued for QA".into(),
            }
        }
        TaskPresentation::Blocked => "blocked".into(),
        TaskPresentation::Paused => "paused".into(),
        TaskPresentation::Terminal => "terminal".into(),
    }
}

fn task_status_text(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::InReview => "in_review",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Paused => "paused",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn complete_wrapup_targets(plans: &[Plan]) -> Vec<(String, String)> {
    plans
        .iter()
        .filter(|plan| crate::service::plan::complete_wrapup::needs_complete_wrapup(plan))
        .filter(|plan| !crate::service::plan::pause_operation::plan_notices_suppressed(plan))
        .filter_map(|plan| {
            let tab = non_empty(plan.owner.tab_id.as_deref())?;
            Some((tab.to_string(), plan.id.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests;
