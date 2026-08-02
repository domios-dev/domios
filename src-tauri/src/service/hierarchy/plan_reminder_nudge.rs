use crate::plan::{Plan, PlanMode, PlanStatus, TaskStatus};
use crate::service::plan::heartbeat::is_plan_complete;
use crate::service::{ServiceContext, ServiceState};
use codeterm_plan::service_plan::state::{task_projection, DependencyWait, TaskPresentation};
use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_PLAN_REMINDER_POLL_SECS: u64 = 60;

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
    use crate::multiplexer::types::AgentRole;
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
            external_refs: vec![],
            test_plan: None,
            qa_plan: None,
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
            execution_intent: Default::default(),
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

    fn aged_item(t: &Task, age_ms: u64) -> crate::plan::reconcile::AgedOpenTask {
        let projection_plan = plan("projection", "orch", PlanMode::Execution, vec![t.clone()]);
        let projection = task_projection(&projection_plan, &projection_plan.tasks[0]);
        crate::plan::reconcile::AgedOpenTask {
            task_id: t.id.clone(),
            name: t.name.clone(),
            status: t.status.clone(),
            presentation: projection.presentation,
            dependency_waits: projection.dependency_waits,
            age_ms,
        }
    }

    #[test]
    fn queued_qa_task_is_not_reported_unjudged() {
        let mut t = task("t_queued", TaskStatus::InReview, None);
        t.review_stage = crate::plan::ReviewStage::Qa;
        t.qa_state = Some(crate::plan::QaState::Queued);
        t.status_label = Some("blocked: reviewer host unavailable overnight".into());
        let item = aged_item(&t, 3 * 3_600_000);

        let text = aged_task_status_text(&item, Some(&t));

        assert_eq!(
            text,
            "queued for QA: blocked: reviewer host unavailable overnight"
        );
        assert!(
            !text.contains("InReview"),
            "must not read as generically unjudged in_review: {text}"
        );
    }

    #[test]
    fn queued_qa_task_without_label_degrades_honestly() {
        let mut t = task("t_queued", TaskStatus::InReview, None);
        t.review_stage = crate::plan::ReviewStage::Qa;
        t.qa_state = Some(crate::plan::QaState::Queued);
        let item = aged_item(&t, 3 * 3_600_000);

        assert_eq!(aged_task_status_text(&item, Some(&t)), "queued for QA");
    }

    #[test]
    fn plain_aged_in_review_task_keeps_its_status_text() {
        let t = task("t_plain", TaskStatus::InReview, None);
        let item = aged_item(&t, 3 * 3_600_000);

        assert_eq!(aged_task_status_text(&item, Some(&t)), "in review");
    }

    #[test]
    fn running_qa_task_is_not_presented_as_queued() {
        let mut t = task("t_running", TaskStatus::InReview, None);
        t.review_stage = crate::plan::ReviewStage::Qa;
        t.qa_state = Some(crate::plan::QaState::Running);
        t.status_label = Some("irrelevant while running".into());
        let item = aged_item(&t, 3 * 3_600_000);

        assert_eq!(aged_task_status_text(&item, Some(&t)), "in review");
    }

    const AGED_NOW_MS: u64 = crate::plan::reconcile::PENDING_UNSTAFFED_STALE_MS + 10;

    fn dependency_fixture() -> Plan {
        let mut active = task("dep_active", TaskStatus::InProgress, Some("worker"));
        active.name = "active dependency".into();
        let mut waiting = task("pending_waiting", TaskStatus::Pending, None);
        waiting.depends_on = vec![active.id.clone()];
        waiting.status_changed_at = 1;
        let mut missing = task("pending_missing", TaskStatus::Pending, None);
        missing.depends_on = vec!["dep_missing".into()];
        missing.status_changed_at = 1;
        let mut actionable = task("pending_free", TaskStatus::Pending, None);
        actionable.name = "orphan work".into();
        actionable.status_changed_at = 1;
        plan(
            "dependency-reminder",
            "orch",
            PlanMode::Execution,
            vec![active, waiting, missing, actionable],
        )
    }

    fn fixture_task<'a>(plan: &'a Plan, id: &str) -> &'a Task {
        plan.tasks.iter().find(|task| task.id == id).unwrap()
    }

    fn is_aged(plan: &Plan, id: &str) -> bool {
        crate::plan::reconcile::aged_open_tasks(plan, AGED_NOW_MS)
            .iter()
            .any(|item| item.task_id == id)
    }

    fn has_stale_unstaffed_drift(plan: &Plan, id: &str) -> bool {
        crate::plan::reconcile::detect_plan_drift(
            plan,
            &crate::plan::reconcile::ReconcileConfig::new(AGED_NOW_MS),
            |_| crate::plan::reconcile::TabProbe::default(),
        )
        .iter()
        .any(|item| {
            item.task_id == id
                && item.kind == crate::plan::reconcile::DriftKind::StalePendingUnstaffed
        })
    }

    fn reminder_key(plan: &Plan) -> String {
        plan_reminder_state_key(
            &plan.id,
            crate::service::plan::reminders::NudgeKind::Orchestrator,
            plan,
        )
    }

    #[test]
    fn dependency_waiting_pending_is_not_stale_unstaffed_or_aged() {
        let plan = dependency_fixture();
        let waiting = fixture_task(&plan, "pending_waiting");
        let projection = task_projection(&plan, waiting);
        assert_eq!(
            projection.presentation,
            TaskPresentation::WaitingOnDependencies
        );
        assert_eq!(projection.dependency_waits.len(), 1);
        assert_eq!(projection.dependency_waits[0].task_id, "dep_active");
        assert_eq!(
            projection.dependency_waits[0].status,
            Some(TaskStatus::InProgress)
        );
        let waiting_item = crate::plan::reconcile::AgedOpenTask {
            task_id: waiting.id.clone(),
            name: waiting.name.clone(),
            status: waiting.status.clone(),
            presentation: projection.presentation,
            dependency_waits: projection.dependency_waits,
            age_ms: AGED_NOW_MS,
        };
        assert_eq!(
            aged_task_status_text(&waiting_item, Some(waiting)),
            "waiting on dependencies: dep_active (in_progress)"
        );

        assert!(!is_aged(&plan, &waiting.id));
        assert!(!has_stale_unstaffed_drift(&plan, &waiting.id));
        assert!(!aged_task_callout_text(&plan, AGED_NOW_MS)
            .unwrap()
            .contains(&waiting.id));
    }

    #[test]
    fn missing_dependency_remains_waiting_and_is_not_stale_actionable() {
        let plan = dependency_fixture();
        let missing = fixture_task(&plan, "pending_missing");
        let projection = task_projection(&plan, missing);
        assert_eq!(
            projection.presentation,
            TaskPresentation::WaitingOnDependencies
        );
        assert_eq!(projection.dependency_waits.len(), 1);
        assert_eq!(projection.dependency_waits[0].task_id, "dep_missing");
        assert!(projection.dependency_waits[0].missing);
        assert_eq!(projection.dependency_waits[0].status, None);

        let waiting_item = crate::plan::reconcile::AgedOpenTask {
            task_id: missing.id.clone(),
            name: missing.name.clone(),
            status: missing.status.clone(),
            presentation: projection.presentation,
            dependency_waits: projection.dependency_waits,
            age_ms: AGED_NOW_MS,
        };
        assert_eq!(
            aged_task_status_text(&waiting_item, Some(missing)),
            "waiting on dependencies: dep_missing (missing)"
        );
        assert!(!is_aged(&plan, &missing.id));
        assert!(!has_stale_unstaffed_drift(&plan, &missing.id));
        let waiting_key = reminder_key(&plan);
        let mut dependency_free = plan.clone();
        dependency_free
            .tasks
            .iter_mut()
            .find(|task| task.id == missing.id)
            .unwrap()
            .depends_on
            .clear();
        assert_ne!(waiting_key, reminder_key(&dependency_free));
    }

    #[test]
    fn dependency_waiting_pending_is_not_dispatched() {
        let plan = dependency_fixture();
        let live_tabs = std::collections::HashSet::from(["worker".to_string()]);
        let snapshot = codeterm_plan::service_plan::state::build_plan_state_snapshot(
            &plan,
            crate::plan::coverage::CoverageReport {
                plan_covered_fraction: 0.0,
                sections: vec![],
                gaps: vec![],
                unlinked_count: 0,
                unlinked_tasks: vec![],
            },
            vec![],
            vec![],
            vec![],
            &live_tabs,
            None,
            AGED_NOW_MS,
        );

        assert!(snapshot.next_actions.iter().all(|action| {
            !matches!(
                action.kind,
                codeterm_plan::service_plan::next_actions::NextActionKind::Continue
            ) && !(action.kind
                == codeterm_plan::service_plan::next_actions::NextActionKind::Dispatch
                && matches!(
                    action.task_id.as_deref(),
                    Some("pending_waiting" | "pending_missing")
                ))
        }));
    }

    #[test]
    fn dependency_free_aged_pending_remains_stale_unstaffed() {
        let plan = dependency_fixture();
        let actionable = fixture_task(&plan, "pending_free");
        assert_eq!(
            task_projection(&plan, actionable).presentation,
            TaskPresentation::ActionablePending
        );
        assert!(is_aged(&plan, &actionable.id));
        assert!(has_stale_unstaffed_drift(&plan, &actionable.id));
        let callout = aged_task_callout_text(&plan, AGED_NOW_MS).unwrap();
        assert_eq!(callout.matches("pending_free").count(), 1);
        assert!(callout.contains("actionable pending"));
    }

    #[test]
    fn dependency_completion_changes_reminder_state_key_without_status_change() {
        let mut plan = dependency_fixture();
        let before = reminder_key(&plan);
        assert_eq!(before, reminder_key(&plan));
        assert!(!aged_task_callout_text(&plan, AGED_NOW_MS)
            .unwrap()
            .contains("pending_waiting"));

        plan.tasks
            .iter_mut()
            .find(|task| task.id == "dep_active")
            .unwrap()
            .status = TaskStatus::Done;
        let waiting = fixture_task(&plan, "pending_waiting");
        assert_eq!(waiting.status, TaskStatus::Pending);
        assert_eq!(
            task_projection(&plan, waiting).presentation,
            TaskPresentation::ActionablePending
        );
        let after = reminder_key(&plan);
        assert_ne!(before, after);
        assert_eq!(after, reminder_key(&plan));
        assert!(aged_task_callout_text(&plan, AGED_NOW_MS)
            .unwrap()
            .contains("pending_waiting"));
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
    fn event_paths_have_no_plan_reminder_injection() {
        let cases = [
            (
                "report completion",
                include_str!("../hierarchy.rs"),
                concat!("maybe_send_", "plan_reminder("),
            ),
            (
                "HTTP owned-tab send",
                include_str!("../../api/tab_routes/send.rs"),
                concat!("maybe_send_owned_", "plan_reminder("),
            ),
            (
                "socket owned-tab send",
                include_str!("../../socket/server/tab.rs"),
                concat!("maybe_send_owned_", "plan_reminder("),
            ),
            (
                "HTTP text polling",
                include_str!("../../api/tab_routes.rs"),
                concat!("maybe_send_polling_", "nudge("),
            ),
            (
                "HTTP chat polling",
                include_str!("../../api/tab_chat_routes.rs"),
                concat!("maybe_send_polling_", "nudge("),
            ),
            (
                "socket output polling",
                include_str!("../../socket/server/tab.rs"),
                concat!("maybe_send_polling_", "nudge("),
            ),
            (
                "idle transition",
                include_str!("../../startup/background.rs"),
                concat!("maybe_send_idle_", "plan_poke("),
            ),
        ];

        for (event, source, obsolete_call) in cases {
            assert!(
                !source.contains(obsolete_call),
                "{event} must not inject a periodic plan reminder"
            );
        }

        let hierarchy = include_str!("../hierarchy.rs");
        assert!(hierarchy.contains("report.inject_body()"));
        assert!(hierarchy.contains("pub fn notify_owner_task_in_review"));
        assert!(hierarchy.contains("pub fn nudge_gate_asker"));
    }

    #[test]
    fn reminder_role_only_shell_records_failure_without_pty_write_or_nudge() {
        let (mux, tab_id) = reminder_test_tab();
        let counter = crate::service::plan::reminders::TurnCounter::new();
        let writes = RefCell::new(Vec::new());
        let plans = vec![owner_plan(&tab_id)];
        assert!(!counter.time_nudge_due(&tab_id, 90, 10));

        send_due_plan_time_reminders_with(&plans, &counter, 100, 10, |id, _, _| {
            if crate::service::plan_reconcile::tab_has_live_agent(mux.get_tab(id).unwrap()) {
                writes.borrow_mut().push(id.to_string());
                Ok(())
            } else {
                Err(
                    "agent_unavailable: plan reminder target has no verified live agent session"
                        .into(),
                )
            }
        });

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
    fn scheduler_delivers_at_1800_seconds_once_and_marks_nudged() {
        let (mut mux, tab_id) = reminder_test_tab();
        let counter = crate::service::plan::reminders::TurnCounter::new();
        let plans = vec![owner_plan(&tab_id)];
        let interval =
            codeterm_types::config::schema::OrchestrationConfig::default().nudge_interval_secs;
        assert_eq!(interval, 1800);
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
        assert!(!counter.time_nudge_due(&tab_id, 0, interval));

        for (now, expected_deliveries) in [(1799, 0), (1800, 1), (1800, 1)] {
            send_due_plan_time_reminders_with(&plans, &counter, now, interval, |id, _, _| {
                assert!(crate::service::plan_reconcile::tab_has_live_agent(
                    mux.get_tab(id).unwrap()
                ));
                writes.borrow_mut().push(id.to_string());
                Ok(())
            });
            assert_eq!(writes.borrow().len(), expected_deliveries);
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
