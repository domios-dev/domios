use crate::multiplexer::types::AgentRole;
use crate::plan::{Plan, PlanMode, PlanStatus, TaskStatus};
use crate::service::plan::heartbeat::is_plan_complete;
use crate::service::{ServiceContext, ServiceState};
use std::collections::BTreeMap;
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
    pane_id: String,
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
        for timed in timed_plan_reminder_targets(&self.plan_store.list_full()) {
            if !self
                .plan_turn_counter
                .time_nudge_due(&timed.pane_id, now, cfg.nudge_interval_secs)
            {
                continue;
            }
            if self.deliver_plan_reminder_target(
                &timed.pane_id,
                &timed.target,
                timed.target.time_nudge_kind(),
            ) {
                self.plan_turn_counter.mark_time_nudged(&timed.pane_id, now);
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
        if let Some(pane_id) = non_empty(plan.owner.pane_id.as_deref()) {
            targets.insert(
                pane_id.to_string(),
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
            let Some(pane_id) = non_empty(task.assigned_pane_id.as_deref()) else {
                continue;
            };
            targets
                .entry(pane_id.to_string())
                .or_insert(PlanReminderTarget::Worker);
        }
    }
    targets
        .into_iter()
        .map(|(pane_id, target)| TimedPlanReminderTarget { pane_id, target })
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
            let pane = non_empty(plan.owner.pane_id.as_deref())?;
            Some((pane.to_string(), plan.id.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{ElementCanvas, OrchestratorRef, Task, TaskLaneMetadata, TaskPriority};

    fn plan(id: &str, owner: &str, mode: PlanMode, tasks: Vec<Task>) -> Plan {
        let mut plan = Plan::new(
            "Reminder".into(),
            "/tmp/project".into(),
            OrchestratorRef::from_pane(owner),
        );
        plan.id = id.into();
        plan.mode = mode;
        plan.tasks = tasks;
        plan
    }

    fn task(id: &str, status: TaskStatus, pane_id: Option<&str>) -> Task {
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
            assigned_pane_id: pane_id.map(str::to_string),
            lane: TaskLaneMetadata::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: ElementCanvas::default(),
            milestones: vec![],
            execution_attempts: vec![],
            park_reason: None,
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
                    pane_id: "orch".into(),
                    target: PlanReminderTarget::Owner {
                        plan_id: "pl_1".into()
                    }
                },
                TimedPlanReminderTarget {
                    pane_id: "worker".into(),
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
    fn time_targets_prioritize_owner_over_worker_for_same_pane() {
        let plans = vec![plan(
            "pl_1",
            "orch",
            PlanMode::Execution,
            vec![task("task_1", TaskStatus::Pending, Some("orch"))],
        )];

        assert_eq!(
            timed_plan_reminder_targets(&plans),
            vec![TimedPlanReminderTarget {
                pane_id: "orch".into(),
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

        assert!(targets.iter().any(|target| target.pane_id == "p1"));
        assert!(targets.iter().any(|target| target.pane_id == "p2"));
        assert!(targets.iter().any(|target| target.pane_id == "p3"));
        assert!(!targets.iter().any(|target| target.pane_id == "p4"));
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
            targets.iter().any(|t| t.pane_id == "orch-review"),
            "in_review open work must keep plan non-complete and owner nudge-eligible"
        );
    }
}
