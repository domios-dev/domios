use crate::multiplexer::types::Tab;
use crate::plan::{Plan, PlanMode, PlanStatus, Task, TaskStatus};
use crate::service::hierarchy::heartbeat_gate::{
    self, Decision, Gate, HardWake, Observation, ReminderKind, Scope, Snapshot, Supporter,
};
use crate::service::plan::heartbeat::is_plan_complete;
use crate::service::plan::reminders::{NudgeKind, TurnCounter};
use crate::service::{ServiceContext, ServiceState};
use codeterm_plan::service_plan::state::{
    orchestrator_actionable_task_ids, task_projection, DependencyWait, TaskPresentation,
};
use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod approval;
#[cfg(test)]
mod approval_tests;

use approval::{Emission, ReminderHost, ReminderRequest, Transport};

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
    counter: &TurnCounter,
    now: u64,
    interval_secs: u64,
    deliver: impl Fn(&str, &PlanReminderTarget, NudgeKind) -> Result<bool, String>,
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
        if !is_due && matches!(timed.target, PlanReminderTarget::Worker) {
            continue;
        }
        due += 1;
        counter.record_time_due(&timed.tab_id, now);
        match deliver(&timed.tab_id, &timed.target, timed.target.time_nudge_kind()) {
            Ok(false) => {}
            Ok(true) => {
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

/// One scheduler pass over every automatic producer: mode change, timed Owner
/// and Worker, and complete-plan wrap-up. Each needs its own Choice approval.
fn run_reminder_tick(
    host: &impl ReminderHost,
    plans: &[Plan],
    counter: &TurnCounter,
    now: u64,
    interval_secs: u64,
) {
    host.gate().retain(&approval::scope_keys(plans));
    for plan in plans {
        let Some(owner) = non_empty(plan.owner.tab_id.as_deref()) else {
            continue;
        };
        let mode = format!("{:?}:{:?}", plan.mode, plan.status);
        if plan.mode != PlanMode::Execution
            && host
                .gate()
                .mode_changed(&approval::scope_key(&plan.id, owner), &mode)
        {
            let request = ReminderRequest::owner(&plan.id, owner, ReminderKind::ModeChange);
            if let Err(error) = deliver_reminder(host, &request, now) {
                log::warn!("[heartbeat-gate] mode transition unavailable: {error}");
            }
        }
    }
    send_due_plan_time_reminders_with(
        plans,
        counter,
        now,
        interval_secs,
        |tab_id, target, kind| deliver_target(host, counter, tab_id, target, kind, now),
    );
    send_due_complete_wrapup_reminders(host, plans, counter, now, interval_secs);
}

fn send_due_complete_wrapup_reminders(
    host: &impl ReminderHost,
    plans: &[Plan],
    counter: &TurnCounter,
    now: u64,
    interval_secs: u64,
) {
    for (tab_id, plan_id) in complete_wrapup_targets(plans) {
        if !counter.complete_wrapup_due(&plan_id)
            || !counter.time_nudge_due(&tab_id, now, interval_secs)
        {
            continue;
        }
        let request = ReminderRequest::owner(&plan_id, &tab_id, ReminderKind::CompleteWrapup);
        match deliver_reminder(host, &request, now) {
            Ok(true) => {
                counter.mark_complete_wrapup_sent(&plan_id);
                counter.mark_time_nudged(&tab_id, now);
            }
            Ok(false) => {}
            Err(reason) => {
                log::warn!("[plan-reminder] complete wrapup not delivered to {tab_id}: {reason}")
            }
        }
    }
}

fn deliver_target(
    host: &impl ReminderHost,
    counter: &TurnCounter,
    tab_id: &str,
    target: &PlanReminderTarget,
    kind: NudgeKind,
    now: u64,
) -> Result<bool, String> {
    ensure_routable(host, tab_id)?;
    let state_key = match target {
        PlanReminderTarget::Owner { plan_id } => host
            .plan(plan_id)
            .map(|plan| plan_reminder_state_key(plan_id, kind, &plan))
            .unwrap_or_else(|| format!("owner:{plan_id}:{kind:?}")),
        PlanReminderTarget::Worker => format!("worker:{tab_id}:{kind:?}"),
    };
    if matches!(target, PlanReminderTarget::Worker)
        && !counter.reminder_content_due(tab_id, &state_key, now)
    {
        return Ok(false);
    }
    let delivered = deliver_reminder(host, &target.request(tab_id, kind), now)?;
    if delivered {
        counter.mark_reminder_content_delivered(tab_id, state_key, now);
    }
    Ok(delivered)
}

fn deliver_reminder(
    host: &impl ReminderHost,
    request: &ReminderRequest,
    now: u64,
) -> Result<bool, String> {
    ensure_routable(host, &request.tab_id)?;
    approval::admit(host, request, now).into_delivery()
}

fn ensure_routable(host: &impl ReminderHost, tab_id: &str) -> Result<(), String> {
    if host
        .tab(tab_id)
        .is_some_and(|tab| crate::service::plan_reconcile::tab_has_live_agent(&tab))
    {
        Ok(())
    } else {
        Err("agent_unavailable: plan reminder target has no verified live agent session".into())
    }
}

struct LiveHost<'a> {
    svc: &'a ServiceContext,
    interval: u64,
    plans: Option<&'a [Plan]>,
}

impl<'a> LiveHost<'a> {
    fn new(svc: &'a ServiceContext) -> Self {
        Self {
            svc,
            interval: crate::config::load_config()
                .orchestration
                .nudge_interval_secs,
            plans: None,
        }
    }
}

impl ReminderHost for LiveHost<'_> {
    fn gate(&self) -> &Gate {
        &self.svc.heartbeat_gate
    }

    fn supporter(&self) -> Supporter<'_> {
        Supporter::live(self.svc)
    }

    fn interval(&self) -> u64 {
        self.interval
    }

    fn plan(&self, plan_id: &str) -> Option<Plan> {
        self.svc.plan_store.get(plan_id)
    }

    fn with_plans<R>(&self, read: impl FnOnce(&[Plan]) -> R) -> R {
        match self.plans {
            Some(plans) => read(plans),
            None => read(&self.svc.plan_store.list_full_reminder_candidates()),
        }
    }

    fn tab(&self, tab_id: &str) -> Option<Tab> {
        let mux = self.svc.mux.read();
        mux.resolve_tab_id(tab_id)
            .and_then(|resolved| mux.get_tab(&resolved))
            .cloned()
    }

    fn observe_owner(
        &self,
        plan: &Plan,
        tab_id: &str,
        previous: &Observation,
        now: u64,
    ) -> (Snapshot, Observation) {
        heartbeat_gate::collect(self.svc, plan, tab_id, previous, now)
    }

    fn compose(
        &self,
        plan: &Plan,
        scope: &Scope,
        snapshot: &Snapshot,
        decision: &Decision,
    ) -> Emission {
        self.svc
            .compose_plan_reminder(plan, scope, snapshot, decision)
    }

    fn emit(&self, plan_id: &str, scope: &Scope, emission: &Emission) -> Result<(), String> {
        let tab_id = scope.tab_id.as_str();
        match &emission.transport {
            Transport::Heartbeat {
                learning_ids,
                plain,
                mode_transition,
            } => crate::service::orchestrator_digests::deliver_gated_heartbeat_exact(
                self.svc,
                plan_id,
                tab_id,
                &emission.text,
                learning_ids.as_deref(),
                *plain,
                *mode_transition,
            )
            .map(|_| {
                self.svc.record_kpi_event(
                    tab_id,
                    crate::agent::kpi_ledger::KpiEventKind::Nudge,
                    emission.text.len(),
                );
            }),
            Transport::Automated => self
                .svc
                .deliver_automated_plan_reminder(tab_id, &emission.text),
        }
    }
}

impl ServiceContext {
    fn send_due_plan_time_reminders(&self) {
        let cfg = crate::config::load_config().orchestration;
        if !cfg.plan_reminders || cfg.nudge_interval_secs == 0 {
            self.plan_turn_counter.clear_time_reminders();
            return;
        }
        let plans = self.plan_store.list_full_reminder_candidates();
        let host = LiveHost {
            svc: self,
            interval: cfg.nudge_interval_secs,
            plans: Some(&plans),
        };
        run_reminder_tick(
            &host,
            &plans,
            &self.plan_turn_counter,
            current_epoch_secs(),
            cfg.nudge_interval_secs,
        );
    }

    /// Lifecycle mutations use this to clear only deadlines that became
    /// ineligible (paused, complete, parked/closed with no open work, or
    /// archived). Active execution owners keep their original deadline.
    pub(crate) fn reconcile_time_reminder_eligibility(&self) {
        let targets = timed_plan_reminder_targets(&self.plan_store.list_full_reminder_candidates());
        self.retain_time_reminder_eligibility(&targets);
    }

    fn retain_time_reminder_eligibility(&self, targets: &[TimedPlanReminderTarget]) {
        let eligible_tabs = targets.iter().map(|target| target.tab_id.clone()).collect();
        self.plan_turn_counter.retain_time_reminders(&eligible_tabs);
    }

    pub(super) fn deliver_plan_reminder_target(
        &self,
        tab_id: &str,
        target: &PlanReminderTarget,
        kind: NudgeKind,
    ) -> Result<bool, String> {
        deliver_target(
            &LiveHost::new(self),
            &self.plan_turn_counter,
            tab_id,
            target,
            kind,
            current_epoch_secs(),
        )
    }

    /// Host-authored reminder text; the model chose only whether it is useful.
    fn compose_plan_reminder(
        &self,
        plan: &Plan,
        scope: &Scope,
        snapshot: &Snapshot,
        decision: &Decision,
    ) -> Emission {
        let tab_id = scope.tab_id.as_str();
        match scope.kind {
            ReminderKind::Worker => Emission {
                text: self.plan_store.prepend_worker_charter_for_tab(
                    tab_id,
                    &crate::service::plan::reminders::worker_nudge_text(),
                ),
                transport: Transport::Automated,
            },
            ReminderKind::CompleteWrapup => {
                let mut text =
                    crate::service::plan::complete_wrapup::complete_wrapup_message(&plan.id);
                // A complete plan gets no heartbeat, so the release map boundary
                // rides the one wrapup this plan is sent.
                for line in crate::service::cartographer::digest_lines(plan) {
                    text.push_str("\n\n");
                    text.push_str(&line);
                }
                Emission {
                    text,
                    transport: Transport::Automated,
                }
            }
            ReminderKind::Owner | ReminderKind::ModeChange | ReminderKind::StalledTeam => {
                let mut text = if scope.kind == ReminderKind::StalledTeam {
                    crate::service::plan::reminders::ORCHESTRATOR_STALLED_TEAM_REMINDER.replace(
                        "{actionable}",
                        &orchestrator_actionable_task_ids(plan).join(", "),
                    )
                } else {
                    crate::service::plan::reminders::orchestrator_nudge_text(plan)
                };
                let mode_transition = snapshot.hard.contains(&HardWake::ModeChange);
                let plain = mode_transition && plan.mode != PlanMode::Execution;
                if plain {
                    text.clear();
                }
                text.push_str("\n\n");
                text.push_str(&decision.next_step_text());
                if !plain {
                    let now_ms = current_epoch_secs().saturating_mul(1000);
                    if let Some(named) = aged_task_callout_text(plan, now_ms) {
                        text.push_str("\n\n");
                        text.push_str(&named);
                    }
                    let resolution = self
                        .plan_store
                        .resolve_planner_tab(&plan.id, |pid| self.tab_probe(pid));
                    if let Some(prefix) =
                        crate::service::plan::peer_mandate::planner_pairing_nudge_prefix(
                            &resolution,
                        )
                    {
                        text = format!("{prefix}\n\n{text}");
                    }
                    // Planner by name (state + binding derived, not tab-guessed).
                    if let Some(defer) =
                        crate::service::plan::peer_mandate::planning_structure_nudge(
                            plan.planning_state,
                            &resolution,
                        )
                    {
                        text = format!("{defer}\n\n{text}");
                    }
                }
                Emission {
                    text,
                    transport: Transport::Heartbeat {
                        learning_ids: decision.learning_ids.clone(),
                        plain,
                        mode_transition,
                    },
                }
            }
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
                e
            })
    }
}

fn plan_reminder_state_key(plan_id: &str, kind: NudgeKind, plan: &Plan) -> String {
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
    fn time_nudge_kind(&self) -> NudgeKind {
        match self {
            Self::Owner { .. } => NudgeKind::Orchestrator,
            Self::Worker => NudgeKind::Worker,
        }
    }

    fn request(&self, tab_id: &str, kind: NudgeKind) -> ReminderRequest {
        match self {
            Self::Owner { plan_id } if kind == NudgeKind::StalledTeam => {
                ReminderRequest::owner(plan_id, tab_id, ReminderKind::StalledTeam)
            }
            Self::Owner { plan_id } => ReminderRequest::owner(plan_id, tab_id, ReminderKind::Owner),
            Self::Worker => ReminderRequest::worker(tab_id),
        }
    }
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn active_execution(plan: &Plan) -> bool {
    plan.status == PlanStatus::Active && plan.mode == PlanMode::Execution && !is_plan_complete(plan)
}

fn owner_reminder_eligible(plan: &Plan) -> bool {
    active_execution(plan)
        && plan
            .tasks
            .iter()
            .any(|task| task.status.counts_as_open_work() && task.status != TaskStatus::Paused)
}

fn worker_reminder_eligible(plan: &Plan, task: &Task) -> bool {
    active_execution(plan)
        && matches!(
            task.status,
            TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Blocked
        )
        && task_projection(plan, task).presentation != TaskPresentation::WaitingOnDependencies
}

pub(crate) fn timed_plan_reminder_targets(plans: &[Plan]) -> Vec<TimedPlanReminderTarget> {
    let mut targets = BTreeMap::<(String, String), PlanReminderTarget>::new();
    for plan in plans.iter().filter(|plan| owner_reminder_eligible(plan)) {
        if let Some(tab_id) = non_empty(plan.owner.tab_id.as_deref()) {
            targets.insert(
                (tab_id.to_string(), plan.id.clone()),
                PlanReminderTarget::Owner {
                    plan_id: plan.id.clone(),
                },
            );
        }
    }
    for plan in plans {
        for task in plan
            .tasks
            .iter()
            .filter(|task| worker_reminder_eligible(plan, task))
        {
            let Some(tab_id) = non_empty(task.assigned_tab_id.as_deref()) else {
                continue;
            };
            if !targets.keys().any(|(owner, _)| owner == tab_id) {
                targets
                    .entry((tab_id.to_string(), String::new()))
                    .or_insert(PlanReminderTarget::Worker);
            }
        }
    }
    targets
        .into_iter()
        .map(|((tab_id, _), target)| TimedPlanReminderTarget { tab_id, target })
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
        TaskPresentation::BlockedOnSubtasks => "waiting on subtasks".into(),
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
