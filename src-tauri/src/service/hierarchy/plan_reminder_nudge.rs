use crate::multiplexer::types::AgentRole;
use crate::service::ServiceContext;

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
        let effective_role = if matches!(target, PlanReminderTarget::Owner { .. }) {
            Some(AgentRole::Orchestrator)
        } else {
            role
        };
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

        let text = crate::service::plan::reminders::nudge_text(kind);
        let text = match target {
            PlanReminderTarget::Owner { plan_id } => {
                self.plan_store.prepend_heartbeat_for_plan(&plan_id, &text)
            }
            PlanReminderTarget::Worker => self
                .plan_store
                .prepend_worker_charter_for_pane(pane_id, &text),
        };
        let envelope = crate::service::messenger::MessageEnvelope::reminder("CodeTerm", "system");
        if crate::service::messenger::Messenger::deliver(self, pane_id, envelope, &text).is_ok() {
            self.plan_turn_counter.mark_nudged(pane_id, turns);
        }
    }
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
}
