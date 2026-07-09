use crate::multiplexer::types::AgentRole;
use crate::service::plan::reminders::TimedPlanReminderKind;
use crate::service::{ServiceContext, ServiceState};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_PLAN_REMINDER_POLL_SECS: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlanReminderScope {
    AnyEligible,
    OwnerOnly,
}

#[derive(Debug, PartialEq, Eq)]
enum PlanReminderTarget {
    Owner { plan_id: String },
    Worker,
}

pub(crate) fn start_plan_reminder_scheduler(svc: ServiceState) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(plan_reminder_poll_secs())).await;
            let svc_for_scan = svc.clone();
            if let Err(e) = tauri::async_runtime::spawn_blocking(move || {
                svc_for_scan.send_due_plan_time_reminders();
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

fn plan_reminder_target(
    task_binding: Option<(String, String)>,
    owner_plan_id: Option<String>,
    role: Option<AgentRole>,
    scope: PlanReminderScope,
) -> Option<PlanReminderTarget> {
    match scope {
        PlanReminderScope::OwnerOnly => {
            owner_plan_id.map(|plan_id| PlanReminderTarget::Owner { plan_id })
        }
        PlanReminderScope::AnyEligible
            if matches!(role, Some(AgentRole::Orchestrator)) && owner_plan_id.is_some() =>
        {
            owner_plan_id.map(|plan_id| PlanReminderTarget::Owner { plan_id })
        }
        PlanReminderScope::AnyEligible if task_binding.is_some() => {
            Some(PlanReminderTarget::Worker)
        }
        PlanReminderScope::AnyEligible => {
            owner_plan_id.map(|plan_id| PlanReminderTarget::Owner { plan_id })
        }
    }
}

impl ServiceContext {
    pub(super) fn maybe_send_plan_reminder(&self, pane_id: &str, role: Option<AgentRole>) {
        self.maybe_send_plan_reminder_for(pane_id, role, PlanReminderScope::AnyEligible);
    }

    pub(crate) fn maybe_send_owned_plan_reminder(&self, pane_id: &str) {
        self.maybe_send_plan_reminder_for(
            pane_id,
            Some(AgentRole::Orchestrator),
            PlanReminderScope::OwnerOnly,
        );
    }

    pub(crate) fn maybe_send_idle_plan_poke(&self, pane_id: &str) {
        let cfg = crate::config::load_config().orchestration;
        if !cfg.plan_reminders || !cfg.idle_poke.enabled {
            return;
        }

        let Some((plan_id, pending_tasks, active_lanes)) = idle_frontier_for_poke(
            self.plan_store.active_owned_plan_frontier_for_pane(pane_id),
            cfg.idle_poke.live_lane_cap,
        ) else {
            return;
        };

        let now = current_epoch_secs();
        if !self
            .plan_turn_counter
            .time_nudge_due(pane_id, now, cfg.nudge_interval_secs)
        {
            return;
        }
        if !self.plan_turn_counter.idle_poke_due(
            pane_id,
            now,
            cfg.idle_poke.max_per_day,
            cfg.idle_poke.backoff_base_secs,
        ) {
            return;
        }

        let phrase_index = self.plan_turn_counter.idle_phrase_index(pane_id);
        let text = crate::service::plan::reminders::idle_poke_text(
            pending_tasks,
            active_lanes,
            phrase_index,
        );
        let text = crate::service::orchestrator_digests::prepend_orchestrator_heartbeat(
            self,
            &plan_id,
            Some(pane_id),
            &text,
        );
        let envelope = crate::service::messenger::MessageEnvelope::reminder("CodeTerm", "system");
        let success =
            crate::service::messenger::Messenger::deliver(self, pane_id, envelope, &text).is_ok();

        if success {
            self.plan_turn_counter.mark_time_nudged(pane_id, now);
            self.plan_turn_counter.mark_idle_nudged(pane_id, now);
        }
    }

    fn maybe_send_plan_reminder_for(
        &self,
        pane_id: &str,
        role: Option<AgentRole>,
        scope: PlanReminderScope,
    ) {
        let task_binding = self
            .plan_store
            .report_pending_task_binding_for_pane(pane_id);
        let owner_plan_id = match scope {
            PlanReminderScope::OwnerOnly => self
                .plan_store
                .active_owned_plan_with_open_work_for_pane(pane_id),
            PlanReminderScope::AnyEligible if matches!(role, Some(AgentRole::Orchestrator)) => self
                .plan_store
                .active_owned_plan_with_open_work_for_pane(pane_id),
            PlanReminderScope::AnyEligible if task_binding.is_none() => self
                .plan_store
                .active_owned_plan_with_open_work_for_pane(pane_id),
            PlanReminderScope::AnyEligible => None,
        };
        let Some(target) = plan_reminder_target(task_binding, owner_plan_id, role, scope) else {
            return;
        };

        let (turns, last_nudge) = self.plan_turn_counter.increment(pane_id);
        let cfg = crate::config::load_config().orchestration;
        let effective_role = target.effective_role(role);
        let nudge_cfg = crate::service::plan::reminders::NudgeConfig {
            plan_reminders: cfg.plan_reminders,
            nudge_after_turns: cfg.nudge_after_turns,
            role: effective_role,
        };
        let Some(kind) =
            crate::service::plan::reminders::should_nudge(turns, last_nudge, &nudge_cfg)
        else {
            return;
        };

        if self.deliver_plan_reminder_target(pane_id, &target, kind) {
            self.plan_turn_counter.mark_nudged(pane_id, turns);
            self.plan_turn_counter
                .mark_time_nudged(pane_id, current_epoch_secs());
        }
    }

    fn send_due_plan_time_reminders(&self) {
        let cfg = crate::config::load_config().orchestration;
        if !cfg.plan_reminders || cfg.nudge_interval_secs == 0 {
            return;
        }
        let now = current_epoch_secs();
        let targets = crate::service::plan::reminders::timed_plan_reminder_targets(
            &self.plan_store.list_full(),
        );
        let candidates = targets.len();
        let mut due = 0usize;
        for timed in targets {
            if !self
                .plan_turn_counter
                .time_nudge_due(&timed.pane_id, now, cfg.nudge_interval_secs)
            {
                continue;
            }
            due += 1;
            let target = PlanReminderTarget::from(timed.kind);
            let delivered = self.deliver_plan_reminder_target(
                &timed.pane_id,
                &target,
                target.time_nudge_kind(),
            );
            // A reminder that silently fails to deliver is indistinguishable
            // from the feature not existing — log both outcomes.
            if delivered {
                log::info!(
                    "[plan-reminder] delivered {:?} nudge to pane {}",
                    target.time_nudge_kind(),
                    timed.pane_id
                );
                self.plan_turn_counter.mark_time_nudged(&timed.pane_id, now);
            } else {
                log::warn!(
                    "[plan-reminder] FAILED to deliver nudge to pane {} (target {:?})",
                    timed.pane_id,
                    target
                );
            }
        }
        self.send_due_complete_wrapup_reminders(now, cfg.nudge_interval_secs);
    }

    fn send_due_complete_wrapup_reminders(&self, now: u64, interval_secs: u64) {
        for (pane_id, plan_id) in complete_wrapup_targets(&self.plan_store.list_full()) {
            if !self.plan_turn_counter.complete_wrapup_due(&plan_id) {
                continue;
            }
            if !self
                .plan_turn_counter
                .time_nudge_due(&pane_id, now, interval_secs)
            {
                continue;
            }
            let text = crate::service::plan::complete_wrapup::complete_wrapup_message(&plan_id);
            let envelope =
                crate::service::messenger::MessageEnvelope::reminder("CodeTerm", "system");
            if crate::service::messenger::Messenger::deliver(self, &pane_id, envelope, &text)
                .is_ok()
            {
                self.plan_turn_counter.mark_complete_wrapup_sent(&plan_id);
                self.plan_turn_counter.mark_time_nudged(&pane_id, now);
            }
        }
    }

    fn deliver_plan_reminder_target(
        &self,
        pane_id: &str,
        target: &PlanReminderTarget,
        kind: crate::service::plan::reminders::NudgeKind,
    ) -> bool {
        let mut text = crate::service::plan::reminders::nudge_text(kind);
        let text = match target {
            PlanReminderTarget::Owner { plan_id } => {
                if let Some(named) = self.aged_task_callout(plan_id) {
                    text.push_str("\n\n");
                    text.push_str(&named);
                }
                let resolution = self
                    .plan_store
                    .resolve_planner_pane(plan_id, |pid| self.pane_probe(pid));
                if let Some(prefix) =
                    crate::service::plan::peer_mandate::planner_pairing_nudge_prefix(&resolution)
                {
                    text = format!("{prefix}\n\n{text}");
                }
                // Planner by name (state + binding derived, not pane-guessed).
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
                crate::service::orchestrator_digests::prepend_orchestrator_heartbeat(
                    self,
                    plan_id,
                    Some(pane_id),
                    &text,
                )
            }
            PlanReminderTarget::Worker => self
                .plan_store
                .prepend_worker_charter_for_pane(pane_id, &text),
        };
        let envelope = crate::service::messenger::MessageEnvelope::reminder("CodeTerm", "system");
        crate::service::messenger::Messenger::deliver(self, pane_id, envelope, &text).is_ok()
    }

    // at concrete drift instead of a generic reminder. None when nothing is aged.
    fn aged_task_callout(&self, plan_id: &str) -> Option<String> {
        let plan = self.plan_store.get(plan_id)?;
        let now = current_epoch_secs().saturating_mul(1000);
        let aged = crate::plan::reconcile::aged_open_tasks(&plan, now);
        if aged.is_empty() {
            return None;
        }
        let listed = aged
            .iter()
            .take(5)
            .map(|t| {
                format!(
                    "- {} \"{}\" ({:?}, aged {}h)",
                    t.task_id,
                    t.name,
                    t.status,
                    t.age_ms / 3_600_000
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!("Stale tasks recorded as drift:\n{listed}"))
    }
}

impl PlanReminderTarget {
    fn effective_role(&self, _fallback: Option<AgentRole>) -> Option<AgentRole> {
        match self {
            Self::Owner { .. } => Some(AgentRole::Orchestrator),
            Self::Worker => Some(AgentRole::Worker),
        }
    }

    fn time_nudge_kind(&self) -> crate::service::plan::reminders::NudgeKind {
        match self {
            Self::Owner { .. } => crate::service::plan::reminders::NudgeKind::Orchestrator,
            Self::Worker => crate::service::plan::reminders::NudgeKind::Worker,
        }
    }
}

impl From<TimedPlanReminderKind> for PlanReminderTarget {
    fn from(kind: TimedPlanReminderKind) -> Self {
        match kind {
            TimedPlanReminderKind::Owner { plan_id } => Self::Owner { plan_id },
            TimedPlanReminderKind::Worker => Self::Worker,
        }
    }
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn idle_frontier_for_poke(
    frontier: Option<(String, usize, usize)>,
    live_lane_cap: u32,
) -> Option<(String, usize, usize)> {
    if live_lane_cap == 0 {
        return None;
    }
    let (plan_id, pending, active_lanes) = frontier?;
    if pending == 0 || active_lanes >= live_lane_cap as usize {
        return None;
    }
    Some((plan_id, pending, active_lanes))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    (!value.is_empty()).then_some(value)
}

fn complete_wrapup_targets(plans: &[Plan]) -> Vec<(String, String)> {
    plans
        .iter()
        .filter(|plan| crate::service::plan::complete_wrapup::needs_complete_wrapup(plan))
        .filter_map(|plan| {
            let pane = non_empty(plan.owner.pane_id.as_deref())?;
            Some((pane.to_string(), plan.id.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_only_trigger_ignores_worker_binding() {
        assert_eq!(
            plan_reminder_target(
                Some(("pl_worker".into(), "task_1".into())),
                None,
                Some(AgentRole::Orchestrator),
                PlanReminderScope::OwnerOnly
            ),
            None
        );
    }

    #[test]
    fn owner_only_trigger_selects_owned_open_plan() {
        assert_eq!(
            plan_reminder_target(
                None,
                Some("pl_owner".into()),
                Some(AgentRole::Orchestrator),
                PlanReminderScope::OwnerOnly
            ),
            Some(PlanReminderTarget::Owner {
                plan_id: "pl_owner".into()
            })
        );
    }

    #[test]
    fn owner_only_trigger_prioritizes_owned_plan_over_task_binding() {
        assert_eq!(
            plan_reminder_target(
                Some(("pl_worker".into(), "task_1".into())),
                Some("pl_owner".into()),
                Some(AgentRole::Orchestrator),
                PlanReminderScope::OwnerOnly
            ),
            Some(PlanReminderTarget::Owner {
                plan_id: "pl_owner".into()
            })
        );
    }

    #[test]
    fn report_path_scope_preserves_worker_reminder_for_worker_pane() {
        assert_eq!(
            plan_reminder_target(
                Some(("pl_worker".into(), "task_1".into())),
                Some("pl_owner".into()),
                Some(AgentRole::Worker),
                PlanReminderScope::AnyEligible
            ),
            Some(PlanReminderTarget::Worker)
        );
    }

    #[test]
    fn report_path_scope_prioritizes_owned_plan_for_orchestrator_pane() {
        assert_eq!(
            plan_reminder_target(
                Some(("pl_worker".into(), "task_1".into())),
                Some("pl_owner".into()),
                Some(AgentRole::Orchestrator),
                PlanReminderScope::AnyEligible
            ),
            Some(PlanReminderTarget::Owner {
                plan_id: "pl_owner".into()
            })
        );
    }

    fn idle_frontier_filters_plan_pokes_by_live_lanes() {
        assert!(idle_frontier_for_poke(Some(("plan-1".into(), 2, 1)), 1).is_none());
        assert_eq!(
            idle_frontier_for_poke(Some(("plan-1".into(), 2, 0)), 1),
            Some(("plan-1".into(), 2, 0))
        );
    }

    #[test]
    fn idle_frontier_blocks_when_no_pending_or_cap_zero() {
        assert!(idle_frontier_for_poke(Some(("plan-1".into(), 0, 0)), 3).is_none());
        assert!(idle_frontier_for_poke(Some(("plan-1".into(), 2, 1)), 0).is_none());
        assert!(idle_frontier_for_poke(None, 3).is_none());
    }
}
