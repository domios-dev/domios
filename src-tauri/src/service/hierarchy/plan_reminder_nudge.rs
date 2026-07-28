use crate::multiplexer::types::AgentRole;
use crate::plan::{Plan, PlanMode, PlanStatus, TaskStatus};
use crate::service::plan::heartbeat::is_plan_complete;
use crate::service::{ServiceContext, ServiceState};
use codeterm_types::pty::TabStatus;
use std::collections::{BTreeMap, HashSet};
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

#[derive(Debug, PartialEq, Eq)]
struct TimedPlanReminderTarget {
    tab_id: String,
    target: PlanReminderTarget,
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

fn send_due_plan_time_reminders_with(
    plans: &[Plan],
    counter: &crate::service::plan::reminders::TurnCounter,
    now: u64,
    interval_secs: u64,
    tab_is_idle: impl Fn(&str) -> bool,
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
        let is_idle_owner =
            matches!(&timed.target, PlanReminderTarget::Owner { .. }) && tab_is_idle(&timed.tab_id);
        let is_due = if is_idle_owner {
            counter.idle_owner_time_nudge_due(&timed.tab_id, now, interval_secs)
        } else {
            counter.time_nudge_due(&timed.tab_id, now, interval_secs)
        };
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
    pub(super) fn maybe_send_plan_reminder(&self, tab_id: &str, role: Option<AgentRole>) {
        self.maybe_send_plan_reminder_for(tab_id, role, PlanReminderScope::AnyEligible);
    }

    pub(crate) fn maybe_send_owned_plan_reminder(&self, tab_id: &str) {
        self.maybe_send_plan_reminder_for(
            tab_id,
            Some(AgentRole::Orchestrator),
            PlanReminderScope::OwnerOnly,
        );
    }

    // A coordinator repeatedly reading a worker/lane tab's output/text/chat is a
    // polling smell — reports auto-inject, so it should wait, not poll.
    pub(crate) fn maybe_send_polling_nudge(&self, caller_tab: &str, target_tab: &str) {
        let caller = caller_tab.trim();
        let target = target_tab.trim();
        if caller.is_empty() || target.is_empty() || caller == target {
            return;
        }
        if !crate::config::load_config().orchestration.plan_reminders {
            return;
        }
        // Gate to coordinator callers polling worker/lane tabs.
        if self
            .plan_store
            .active_owned_plan_with_open_work_for_tab(caller)
            .is_none()
        {
            return;
        }
        if self.plan_store.task_binding_for_tab(target).is_none() {
            return;
        }
        if !self.plan_turn_counter.record_poll(caller) {
            return;
        }
        if let Err(reason) = self.ensure_plan_reminder_routable(caller) {
            log::warn!("[plan-reminder] polling nudge not delivered to {caller}: {reason}");
            return;
        }
        let text = crate::service::plan::reminders::polling_nudge_text();
        let _ = self.deliver_automated_plan_reminder(caller, &text);
    }

    pub(crate) fn maybe_send_idle_plan_poke(&self, tab_id: &str) {
        let cfg = crate::config::load_config().orchestration;
        if !cfg.plan_reminders || !cfg.idle_poke.enabled {
            return;
        }

        let Some((plan_id, pending_tasks, active_lanes)) = idle_frontier_for_poke(
            self.plan_store.active_owned_plan_frontier_for_tab(tab_id),
            cfg.idle_poke.live_lane_cap,
        ) else {
            return;
        };

        let now = current_epoch_secs();
        if !self
            .plan_turn_counter
            .time_nudge_due(tab_id, now, cfg.nudge_interval_secs)
        {
            return;
        }
        if !self.plan_turn_counter.idle_poke_due(
            tab_id,
            now,
            cfg.idle_poke.max_per_day,
            cfg.idle_poke.backoff_base_secs,
        ) {
            return;
        }
        if let Err(reason) = self.ensure_plan_reminder_routable(tab_id) {
            log::warn!("[plan-reminder] idle poke not delivered to {tab_id}: {reason}");
            return;
        }

        let phrase_index = self.plan_turn_counter.idle_phrase_index(tab_id);
        let text = crate::service::plan::reminders::idle_poke_text(
            pending_tasks,
            active_lanes,
            phrase_index,
        );
        let text = crate::service::orchestrator_digests::prepend_orchestrator_heartbeat(
            self,
            &plan_id,
            Some(tab_id),
            &text,
        );
        let success = self.deliver_automated_plan_reminder(tab_id, &text).is_ok();

        if success {
            self.plan_turn_counter.mark_time_nudged(tab_id, now);
            self.plan_turn_counter.mark_idle_nudged(tab_id, now);
        }
    }

    fn maybe_send_plan_reminder_for(
        &self,
        tab_id: &str,
        role: Option<AgentRole>,
        scope: PlanReminderScope,
    ) {
        let task_binding = self.plan_store.report_pending_task_binding_for_tab(tab_id);
        let owner_plan_id = match scope {
            PlanReminderScope::OwnerOnly => self
                .plan_store
                .active_owned_plan_with_open_work_for_tab(tab_id),
            PlanReminderScope::AnyEligible if matches!(role, Some(AgentRole::Orchestrator)) => self
                .plan_store
                .active_owned_plan_with_open_work_for_tab(tab_id),
            PlanReminderScope::AnyEligible if task_binding.is_none() => self
                .plan_store
                .active_owned_plan_with_open_work_for_tab(tab_id),
            PlanReminderScope::AnyEligible => None,
        };
        let Some(target) = plan_reminder_target(task_binding, owner_plan_id, role, scope) else {
            return;
        };

        let (turns, last_nudge) = self.plan_turn_counter.increment(tab_id);
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

        if self
            .deliver_plan_reminder_target(tab_id, &target, kind)
            .is_ok()
        {
            self.plan_turn_counter.mark_nudged(tab_id, turns);
            self.plan_turn_counter
                .mark_time_nudged(tab_id, current_epoch_secs());
        }
    }

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
            |tab_id| self.plan_reminder_tab_is_idle(tab_id),
            |tab_id, target, kind| self.deliver_plan_reminder_target(tab_id, target, kind),
        );
        self.send_due_complete_wrapup_reminders(now, cfg.nudge_interval_secs);
    }

    fn plan_reminder_tab_is_idle(&self, tab_id: &str) -> bool {
        let pty_id = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).map(|tab| tab.pty_id.clone())
        };
        match pty_id {
            Some(Some(pty_id)) => self.agent_tracker.lock().get_status(&pty_id) == TabStatus::Idle,
            Some(None) => true,
            None => false,
        }
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
        let mut text = crate::service::plan::reminders::nudge_text(kind);
        let text = match target {
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
                crate::service::orchestrator_digests::prepend_orchestrator_heartbeat(
                    self,
                    plan_id,
                    Some(tab_id),
                    &text,
                )
            }
            PlanReminderTarget::Worker => self
                .plan_store
                .prepend_worker_charter_for_tab(tab_id, &text),
        };
        self.deliver_automated_plan_reminder(tab_id, &text)
            .map(|()| {
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

fn plan_reminder_state_key(
    plan_id: &str,
    kind: crate::service::plan::reminders::NudgeKind,
    plan: &Plan,
) -> String {
    let tasks = plan
        .tasks
        .iter()
        .map(|task| format!("{}:{:?}", task.id, task.status))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "owner:{plan_id}:{kind:?}:{:?}:{:?}:{tasks}",
        plan.status, plan.mode
    )
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

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn timed_plan_reminder_targets(plans: &[Plan]) -> Vec<TimedPlanReminderTarget> {
    let mut targets = BTreeMap::<String, PlanReminderTarget>::new();
    for plan in plans.iter().filter(|plan| {
        plan.status == PlanStatus::Active
            && plan.mode == PlanMode::Execution
            && !is_plan_complete(plan)
            && plan
                .tasks
                .iter()
                .any(|task| task.status.counts_as_open_work())
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
            )
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
            let tab = non_empty(plan.owner.tab_id.as_deref())?;
            Some((tab.to_string(), plan.id.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{ElementCanvas, OrchestratorRef, Task, TaskLaneMetadata, TaskPriority};
    use codeterm_types::pty::BootStage;
    use std::cell::RefCell;

    fn reminder_test_tab() -> (MultiplexerState, String) {
        let (mut mux, tab_id) = MultiplexerState::new();
        mux.set_tab_role(&tab_id, AgentRole::Orchestrator, None)
            .unwrap();
        (mux, tab_id)
    }

    fn plan(id: &str, owner: &str, mode: PlanMode, tasks: Vec<Task>) -> Plan {
        let mut plan = Plan::new(
            "Reminder".into(),
            "/tmp/project".into(),
            OrchestratorRef::from_tab(owner),
        );
        plan.id = id.into();
        plan.mode = mode;
        plan.tasks = tasks;
        plan
    }

    fn task(id: &str, status: TaskStatus, tab_id: Option<&str>) -> Task {
        Task {
            id: id.into(),
            name: id.into(),
            kind: "status_update".into(),
            model: None,
            model_rationale: None,
            acceptance_criteria: vec![],
            test_plan: None,
            correctness: None,
            status,
            review_stage: Default::default(),
            qa_state: None,
            review_round: 0,
            review_oracle: None,
            review_evidence: vec![],
            review_override: None,
            review_audit: vec![],
            status_label: None,
            priority: TaskPriority::P2,
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec![],
            section_ids: vec![],
            body: String::new(),
            role_guidance: None,
            executor_role: None,
            competition: None,
            intent: None,
            parent_task_id: None,
            depends_on: vec![],
            assigned_tab_id: tab_id.map(str::to_string),
            lane: TaskLaneMetadata::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: ElementCanvas::default(),
            milestones: vec![],
            execution_attempts: vec![],
            pause_reason: None,
            personality: None,
            personality_provenance: None,
            attachments: vec![],
        }
    }

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
    fn report_path_scope_preserves_worker_reminder_for_worker_tab() {
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
    fn report_path_scope_prioritizes_owned_plan_for_orchestrator_tab() {
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

    fn owner_plan(tab_id: &str) -> Plan {
        plan(
            "plan-reminder-test",
            tab_id,
            PlanMode::Execution,
            vec![task("open", TaskStatus::Pending, None)],
        )
    }

    #[test]
    fn reminder_state_key_ignores_bookkeeping_timestamp_changes() {
        let mut plan = owner_plan("orch");
        let before = plan_reminder_state_key(
            &plan.id,
            crate::service::plan::reminders::NudgeKind::Orchestrator,
            &plan,
        );
        plan.updated_at = 999_999;
        let after = plan_reminder_state_key(
            &plan.id,
            crate::service::plan::reminders::NudgeKind::Orchestrator,
            &plan,
        );

        assert_eq!(before, after);
    }

    #[test]
    fn reminder_role_only_shell_records_failure_without_pty_write_or_nudge() {
        let (mux, tab_id) = reminder_test_tab();
        let counter = crate::service::plan::reminders::TurnCounter::new();
        let writes = RefCell::new(Vec::new());
        let plans = vec![owner_plan(&tab_id)];

        send_due_plan_time_reminders_with(
            &plans,
            &counter,
            100,
            10,
            |_| true,
            |id, _, _| {
                if crate::service::plan_reconcile::tab_has_live_agent(mux.get_tab(id).unwrap()) {
                    writes.borrow_mut().push(id.to_string());
                    Ok(())
                } else {
                    Err("agent_unavailable: plan reminder target has no verified live agent session".into())
                }
            },
        );

        let telemetry = counter.time_reminder_telemetry(&tab_id);
        assert!(writes.borrow().is_empty());
        assert!(telemetry.last_due.is_some());
        assert!(telemetry.last_delivery.is_none());
        assert!(telemetry
            .delivery_failure_retry_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("verified live agent session")));
        assert!(counter.time_nudge_due(&tab_id, 100, 10));
    }

    #[test]
    fn reminder_session_verified_agent_delivers_once_and_marks_nudged() {
        let (mut mux, tab_id) = reminder_test_tab();
        let counter = crate::service::plan::reminders::TurnCounter::new();
        let plans = vec![owner_plan(&tab_id)];
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some("test-provider".into());
            tab.agent_session_id = Some("session-1".into());
            tab.session_binding_nonce = Some("nonce-1".into());
            tab.reported_session_binding_nonce = Some("nonce-1".into());
            tab.pty_id = Some("recording-pty".into());
            tab.chat_view = true;
            tab.boot_stage = Some(BootStage::AgentReady);
        }
        let writes = RefCell::new(Vec::new());

        for _ in 0..2 {
            send_due_plan_time_reminders_with(
                &plans,
                &counter,
                100,
                10,
                |_| true,
                |id, _, _| {
                    assert!(crate::service::plan_reconcile::tab_has_live_agent(
                        mux.get_tab(id).unwrap()
                    ));
                    writes.borrow_mut().push(id.to_string());
                    Ok(())
                },
            );
        }

        let telemetry = counter.time_reminder_telemetry(&tab_id);
        assert_eq!(writes.borrow().len(), 1);
        assert!(telemetry.last_delivery.is_some());
        assert!(telemetry.delivery_failure_retry_reason.is_none());
    }

    #[test]
    fn time_targets_include_execution_owner_and_worker() {
        let plans = vec![plan(
            "pl_1",
            "orch",
            PlanMode::Execution,
            vec![task("task_1", TaskStatus::Pending, Some("worker"))],
        )];

        assert_eq!(
            timed_plan_reminder_targets(&plans),
            vec![
                TimedPlanReminderTarget {
                    tab_id: "orch".into(),
                    target: PlanReminderTarget::Owner {
                        plan_id: "pl_1".into()
                    }
                },
                TimedPlanReminderTarget {
                    tab_id: "worker".into(),
                    target: PlanReminderTarget::Worker
                }
            ]
        );
    }

    #[test]
    fn time_targets_skip_planning_and_terminal_work() {
        let plans = vec![
            plan(
                "pl_planning",
                "orch",
                PlanMode::Planning,
                vec![task("task_1", TaskStatus::Pending, Some("worker"))],
            ),
            plan(
                "pl_done",
                "done-orch",
                PlanMode::Execution,
                vec![task("task_2", TaskStatus::Done, Some("done-worker"))],
            ),
            plan(
                "pl_cancelled",
                "cancelled-orch",
                PlanMode::Execution,
                vec![task(
                    "task_3",
                    TaskStatus::Cancelled,
                    Some("cancelled-worker"),
                )],
            ),
        ];

        assert!(timed_plan_reminder_targets(&plans).is_empty());
    }

    #[test]
    fn time_targets_preserve_pause_completion_and_closure_suppression() {
        let mut paused = plan(
            "pl_paused",
            "paused-owner",
            PlanMode::Execution,
            vec![task("open", TaskStatus::Pending, Some("paused-worker"))],
        );
        paused.mode = PlanMode::Paused;

        let paused_task = plan(
            "pl_paused_task",
            "paused-task-owner",
            PlanMode::Execution,
            vec![task("paused", TaskStatus::Paused, Some("paused-worker"))],
        );

        let mut archived = plan(
            "pl_closed",
            "closed-owner",
            PlanMode::Execution,
            vec![task("open", TaskStatus::Pending, Some("closed-worker"))],
        );
        archived.status = PlanStatus::Archived;

        let no_open_tasks = plan(
            "pl_complete",
            "complete-owner",
            PlanMode::Execution,
            vec![task("done", TaskStatus::Done, Some("done-worker"))],
        );

        assert!(
            timed_plan_reminder_targets(&[paused, paused_task, archived, no_open_tasks]).is_empty()
        );
    }

    #[test]
    fn time_targets_prioritize_owner_over_worker_for_same_tab() {
        let plans = vec![plan(
            "pl_1",
            "orch",
            PlanMode::Execution,
            vec![task("task_1", TaskStatus::Pending, Some("orch"))],
        )];

        assert_eq!(
            timed_plan_reminder_targets(&plans),
            vec![TimedPlanReminderTarget {
                tab_id: "orch".into(),
                target: PlanReminderTarget::Owner {
                    plan_id: "pl_1".into()
                }
            }]
        );
    }

    #[test]
    fn time_targets_follow_reseeded_owner_tab() {
        let mut reseeded = plan(
            "pl_1",
            "stale-owner",
            PlanMode::Execution,
            vec![task("task_1", TaskStatus::InReview, None)],
        );
        reseeded.owner = OrchestratorRef::from_tab("live-owner");

        assert_eq!(
            timed_plan_reminder_targets(&[reseeded]),
            vec![TimedPlanReminderTarget {
                tab_id: "live-owner".into(),
                target: PlanReminderTarget::Owner {
                    plan_id: "pl_1".into()
                }
            }]
        );
    }

    #[test]
    fn time_targets_workers_only_for_report_pending_statuses() {
        let plans = vec![plan(
            "pl_1",
            "orch",
            PlanMode::Execution,
            vec![
                task("pending", TaskStatus::Pending, Some("p1")),
                task("working", TaskStatus::InProgress, Some("p2")),
                task("blocked", TaskStatus::Blocked, Some("p3")),
                task("review", TaskStatus::InReview, Some("p4")),
            ],
        )];
        let targets = timed_plan_reminder_targets(&plans);

        assert!(targets.iter().any(|target| target.tab_id == "p1"));
        assert!(targets.iter().any(|target| target.tab_id == "p2"));
        assert!(targets.iter().any(|target| target.tab_id == "p3"));
        assert!(!targets.iter().any(|target| target.tab_id == "p4"));
    }

    #[test]
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

    #[test]
    fn timed_targets_silent_for_all_done_and_complete_mode_plans() {
        // Execution plan where all tasks are Done — no nudges for owner or workers.
        let all_done = plan(
            "pl_done",
            "orch-done",
            PlanMode::Execution,
            vec![
                task("t1", TaskStatus::Done, Some("w1")),
                task("t2", TaskStatus::Done, Some("w2")),
                task("t3", TaskStatus::Cancelled, Some("w3")),
            ],
        );
        // Complete mode plan with open-looking task — still silent.
        let mut complete_mode = plan(
            "pl_complete",
            "orch-complete",
            PlanMode::Execution,
            vec![task("t4", TaskStatus::Done, None)],
        );
        complete_mode.mode = PlanMode::Complete;

        let targets = timed_plan_reminder_targets(&[all_done, complete_mode]);
        assert!(
            targets.is_empty(),
            "complete plans must produce zero timed reminder targets: {targets:?}"
        );
    }

    #[test]
    fn timed_targets_fire_for_in_review_open_work() {
        // InReview is open work: plan is NOT complete, owner should be nudged.
        let plans = vec![plan(
            "pl_review",
            "orch-review",
            PlanMode::Execution,
            vec![
                task("t_done", TaskStatus::Done, None),
                task("t_review", TaskStatus::InReview, Some("worker-review")),
            ],
        )];
        let targets = timed_plan_reminder_targets(&plans);
        assert!(
            targets.iter().any(|t| t.tab_id == "orch-review"),
            "in_review open work must keep plan non-complete and owner nudge-eligible"
        );
    }
}
