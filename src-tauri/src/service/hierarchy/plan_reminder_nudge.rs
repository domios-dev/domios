use crate::multiplexer::types::AgentRole;
use crate::service::ServiceContext;

impl ServiceContext {
    pub(super) fn maybe_send_plan_reminder(&self, pane_id: &str, role: Option<AgentRole>) {
        let task_binding = self.plan_store.task_binding_for_pane(pane_id);
        let owner_plan_id = task_binding
            .is_none()
            .then(|| {
                self.plan_store
                    .active_owned_plan_with_open_work_for_pane(pane_id)
            })
            .flatten();
        if task_binding.is_none() && owner_plan_id.is_none() {
            return;
        }

        let (turns, last_nudge) = self.plan_turn_counter.increment(pane_id);
        let cfg = crate::config::load_config().orchestration;
        let effective_role = if owner_plan_id.is_some() {
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
        let text = if let Some(plan_id) = owner_plan_id {
            self.plan_store.prepend_heartbeat_for_plan(&plan_id, &text)
        } else {
            self.plan_store
                .prepend_worker_charter_for_pane(pane_id, &text)
        };
        let envelope = crate::service::messenger::MessageEnvelope::reminder("CodeTerm", "system");
        if crate::service::messenger::Messenger::deliver(self, pane_id, envelope, &text).is_ok() {
            self.plan_turn_counter.mark_nudged(pane_id, turns);
        }
    }
}
