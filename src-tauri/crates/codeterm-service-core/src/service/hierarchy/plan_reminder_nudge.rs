use crate::plan::{Plan, PlanMode, PlanStatus, Task, TaskStatus};
use crate::service::hierarchy::heartbeat_gate::{
    self, Decision, Gate, HardWake, Observation, ReminderKind, Scope, Snapshot, TickFact,
};
use crate::service::plan::heartbeat::is_plan_complete;
use crate::service::plan::reminders::{NudgeKind, TurnCounter};
use crate::service::{ServiceContext, ServiceState};
use codeterm_plan::service_plan::state::{
    orchestrator_actionable_task_ids, task_projection, DependencyWait, TaskPresentation,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod approval;
#[cfg(test)]
mod approval_tests;
mod projection;

pub(crate) use projection::{project_for_tab, ProjectedHeartbeat, ProjectedTick};

use approval::{Emission, Recipient, ReminderHost, ReminderRequest, Transport};

const MAX_PLAN_REMINDER_POLL_SECS: u64 = 60;

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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BlockedNotice {
    plan_id: String,
    event_key: String,
    summary: String,
    task_ids: Vec<String>,
    pause_plan: bool,
    actions: Vec<codeterm_notification_core::contract::CallerAction>,
}

pub(crate) fn start_plan_reminder_scheduler(svc: ServiceState) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(plan_reminder_poll_secs(
                &svc.configuration().orchestration,
            )))
            .await;
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

fn plan_reminder_poll_secs(cfg: &crate::config::schema::OrchestrationConfig) -> u64 {
    cfg.nudge_interval_secs
        .clamp(1, MAX_PLAN_REMINDER_POLL_SECS)
}

fn send_due_plan_time_reminders_noting(
    plans: &[Plan],
    counter: &TurnCounter,
    now: u64,
    interval_secs: u64,
    deliver: impl Fn(&str, &PlanReminderTarget, NudgeKind) -> Result<bool, String>,
    note: impl Fn(&str, &PlanReminderTarget, TickFact),
) {
    let targets = timed_plan_reminder_targets(plans);
    counter.retain_time_reminders(&deadline_tabs(plans, counter));
    let candidates = targets.len();
    let mut due = 0usize;
    for timed in targets {
        counter.record_time_tick(&timed.tab_id, now);
        let is_due = counter.time_nudge_due(&timed.tab_id, now, interval_secs);
        if !is_due && matches!(timed.target, PlanReminderTarget::Worker) {
            note(&timed.tab_id, &timed.target, TickFact::NotDue);
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
/// and Worker, and complete-plan wrap-up. Each uses the deterministic gate.
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
            let mut request = ReminderRequest::owner(&plan.id, owner, ReminderKind::ModeChange);
            request.delivery_observer = Some(
                crate::service::messenger::DeliveryWriteObserver::PlanReminder {
                    plan_id: Some(plan.id.clone()),
                    tab_id: owner.to_string(),
                    reminder_kind: "mode_change".into(),
                    state_key: Some(mode.clone()),
                    complete_wrapup_plan_id: None,
                },
            );
            if let Err(error) = deliver_reminder(host, &request, now) {
                log::warn!("[heartbeat-gate] mode transition unavailable: {error}");
            }
        }
    }
    send_due_plan_time_reminders_noting(
        plans,
        counter,
        now,
        interval_secs,
        |tab_id, target, kind| deliver_target(host, counter, tab_id, target, kind, now),
        |tab_id, target, fact| record_target_tick(host, tab_id, target, now, fact),
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
        let mut request = ReminderRequest::owner(&plan_id, &tab_id, ReminderKind::CompleteWrapup);
        request.delivery_observer = Some(
            crate::service::messenger::DeliveryWriteObserver::PlanReminder {
                plan_id: Some(plan_id.clone()),
                tab_id: tab_id.clone(),
                reminder_kind: "complete_wrapup".into(),
                state_key: None,
                complete_wrapup_plan_id: Some(plan_id.clone()),
            },
        );
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
    if let Err(error) = ensure_routable(host, tab_id) {
        record_target_tick(host, tab_id, target, now, TickFact::Unavailable);
        return Err(error);
    }
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
        record_target_tick(host, tab_id, target, now, TickFact::UnchangedContent);
        return Ok(false);
    }
    let mut request = target.request(tab_id, kind);
    let plan_id = match target {
        PlanReminderTarget::Owner { plan_id } => Some(plan_id.clone()),
        PlanReminderTarget::Worker => None,
    };
    request.delivery_observer = Some(
        crate::service::messenger::DeliveryWriteObserver::PlanReminder {
            plan_id,
            tab_id: tab_id.to_string(),
            reminder_kind: format!("{kind:?}"),
            state_key: Some(state_key.clone()),
            complete_wrapup_plan_id: None,
        },
    );
    let delivered = deliver_reminder(host, &request, now)?;
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
    if let Err(error) = ensure_routable(host, &request.tab_id) {
        host.gate().record_tick(heartbeat_gate::delivery_tick(
            &request.tab_id,
            request.plan_id.as_deref().unwrap_or(""),
            now,
            TickFact::Unavailable,
        ));
        return Err(error);
    }
    approval::admit(host, request, now).into_delivery()
}

fn record_target_tick(
    host: &impl ReminderHost,
    tab_id: &str,
    target: &PlanReminderTarget,
    now: u64,
    fact: TickFact,
) {
    let plan_id = match target {
        PlanReminderTarget::Owner { plan_id } => plan_id.as_str(),
        PlanReminderTarget::Worker => "",
    };
    host.gate()
        .record_tick(heartbeat_gate::delivery_tick(tab_id, plan_id, now, fact));
}

fn ensure_routable(host: &impl ReminderHost, tab_id: &str) -> Result<(), String> {
    if host
        .recipient(tab_id)
        .is_some_and(|recipient| crate::service::plan_reconcile::tab_has_live_agent(&recipient.tab))
    {
        Ok(())
    } else {
        Err("agent_unavailable: plan reminder target has no verified live agent session".into())
    }
}

fn blocked_notices(plan: &Plan, scope: &Scope, snapshot: &Snapshot) -> Vec<BlockedNotice> {
    let mut blockers = BTreeMap::<String, BlockerGroup>::new();
    for task in &plan.tasks {
        let projection = task_projection(plan, task);
        if task.status == TaskStatus::Blocked {
            let reason = non_empty(task.pause_reason.as_deref())
                .or_else(|| non_empty(task.status_label.as_deref()))
                .unwrap_or(&task.id);
            record_blocker(
                &mut blockers,
                ("task_blocked", reason),
                &task.id,
                &format!("Task {} is blocked", task.id),
            );
        }
        for wait in &projection.dependency_waits {
            record_blocker(
                &mut blockers,
                (
                    "dependency",
                    wait.task_id.as_str(),
                    wait.status
                        .as_ref()
                        .map(TaskStatus::key)
                        .unwrap_or("missing"),
                ),
                &task.id,
                &format!("Dependency {} is not ready", wait.task_id),
            );
        }
        if task.interruption.handoff_required {
            record_blocker(
                &mut blockers,
                ("handoff", task.id.as_str()),
                &task.id,
                &format!("Task {} needs a handoff", task.id),
            );
        }
        if crate::plan::reconcile::active_review_hold_note(task).is_some() {
            let hold_id = task
                .review_evidence
                .last()
                .map(|evidence| evidence.id.as_str())
                .unwrap_or("");
            record_blocker(
                &mut blockers,
                ("review_hold", task.id.as_str(), hold_id),
                &task.id,
                &format!("Task {} is held for review", task.id),
            );
        }
        for attempt in &task.execution_attempts {
            if let Some(reason) = attempt.interruption.reason.as_deref() {
                record_blocker(
                    &mut blockers,
                    ("machine_hold", reason),
                    &task.id,
                    &format!("Worker on task {} is interrupted", task.id),
                );
            }
        }
    }
    if let Some(task_id) = scope.task_id.as_deref() {
        blockers.retain(|_, blocker| blocker.task_ids.contains(task_id));
    }
    if blockers.is_empty() {
        let task_id = scope.task_id.as_deref().unwrap_or("");
        let recipient_state = if task_id.is_empty() {
            Vec::new()
        } else {
            snapshot
                .projection
                .iter()
                .filter(|(key, _)| key.as_str() != "mode")
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect()
        };
        let owner_messages: Vec<_> = snapshot
            .hard
            .iter()
            .filter_map(|hard| match hard {
                HardWake::OwnerMessage(id) => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let mut task_ids = BTreeSet::new();
        if !task_id.is_empty() {
            task_ids.insert(task_id.to_string());
        }
        blockers.insert(
            crate::service::supporter::criteria_hash(
                &serde_json::to_vec(&("unclassified", task_id, recipient_state, owner_messages))
                    .unwrap_or_default(),
            ),
            BlockerGroup {
                summary: if task_id.is_empty() {
                    "The plan has a blocked recipient".into()
                } else {
                    format!("Task {task_id} cannot act now")
                },
                task_ids,
            },
        );
    }
    let actionable: HashSet<_> = orchestrator_actionable_task_ids(plan).into_iter().collect();
    blockers
        .into_iter()
        .map(|(identity, blocker)| {
            let pause_plan =
                !actionable.is_empty() && actionable.iter().all(|id| blocker.task_ids.contains(id));
            BlockedNotice {
                plan_id: plan.id.clone(),
                event_key: format!("plan-blocker:{}:{identity}", plan.id),
                summary: blocker.summary,
                task_ids: blocker.task_ids.into_iter().collect(),
                pause_plan,
                actions: blocked_notice_actions(),
            }
        })
        .collect()
}

#[derive(Default)]
struct BlockerGroup {
    summary: String,
    task_ids: BTreeSet<String>,
}

fn record_blocker(
    blockers: &mut BTreeMap<String, BlockerGroup>,
    identity: impl serde::Serialize,
    task_id: &str,
    summary: &str,
) {
    let key = crate::service::supporter::criteria_hash(
        &serde_json::to_vec(&identity).unwrap_or_default(),
    );
    let blocker = blockers.entry(key).or_insert_with(|| BlockerGroup {
        summary: summary.into(),
        ..Default::default()
    });
    blocker.task_ids.insert(task_id.into());
}

fn blocked_notice_actions() -> Vec<codeterm_notification_core::contract::CallerAction> {
    ["wait", "pause plan", "decide"]
        .into_iter()
        .map(
            |label| codeterm_notification_core::contract::CallerAction::Answer {
                label: label.into(),
            },
        )
        .collect()
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
            interval: svc.configuration().orchestration.nudge_interval_secs,
            plans: None,
        }
    }
}

impl ReminderHost for LiveHost<'_> {
    fn gate(&self) -> &Gate {
        self.svc
            .ext::<std::sync::Arc<crate::service::hierarchy::heartbeat_gate::Gate>>()
    }

    fn journal_runtime(&self) -> std::sync::Arc<crate::service::supporter::SupporterRuntime> {
        crate::service::supporter::runtime_with_notifications(self.svc)
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

    fn recipient(&self, tab_id: &str) -> Option<Recipient> {
        let mux = self.svc.mux.read();
        let resolved = mux.resolve_tab_id(tab_id)?;
        Some(Recipient {
            tab: mux.get_tab(&resolved)?.clone(),
            workspace_id: mux.find_workspace_for_tab(&resolved),
        })
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

    fn observe_worker(
        &self,
        plan: &Plan,
        scope: &Scope,
        previous: &Observation,
        _: u64,
    ) -> (Snapshot, Observation) {
        let (mut snapshot, mut observed) =
            heartbeat_gate::collect_worker_scope(plan, scope, previous);
        snapshot.hard = heartbeat_gate::actionable_pending(&previous.pending_hard);
        let (_, owner_observation) =
            heartbeat_gate::collect_owner_messages(self.svc, &scope.tab_id, previous);
        if let Some(owner_observation) = owner_observation {
            observed
                .workers
                .insert(scope.tab_id.clone(), owner_observation);
        }
        observed.pending_hard = snapshot.hard.clone();
        (snapshot, observed)
    }

    fn compose(
        &self,
        plan: &Plan,
        scope: &Scope,
        snapshot: &Snapshot,
        decision: &Decision,
    ) -> Emission {
        self.svc
            .compose_plan_reminder(plan, scope, snapshot, decision, false)
    }

    fn emit(
        &self,
        plan_id: &str,
        scope: &Scope,
        emission: &Emission,
        observer: Option<crate::service::messenger::DeliveryWriteObserver>,
    ) -> Result<bool, String> {
        let tab_id = scope.tab_id.as_str();
        self.svc
            .ext::<std::sync::Arc<crate::service::hierarchy::heartbeat_gate::Gate>>()
            .remember_body(tab_id, plan_id, &emission.text);
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
                observer,
            )
            .map(|delivery| {
                if delivery.is_written() {
                    self.svc.record_kpi_event(
                        tab_id,
                        crate::agent::kpi_ledger::KpiEventKind::Nudge,
                        emission.text.len(),
                    );
                }
                delivery.is_written()
            }),
            Transport::Automated => {
                self.svc
                    .deliver_automated_plan_reminder(tab_id, &emission.text, observer)
            }
        }
    }

    fn notify_blocked(&self, owner_tab: &str, notice: &BlockedNotice) {
        self.svc.notify_blocked(owner_tab, notice);
    }
}

impl ServiceContext {
    pub(crate) fn heartbeat_body_for_popup(
        &self,
        plan: &Plan,
        tab_id: &str,
        worker: bool,
    ) -> String {
        if let Some(text) = self
            .ext::<std::sync::Arc<crate::service::hierarchy::heartbeat_gate::Gate>>()
            .last_body(tab_id, &plan.id)
        {
            return text;
        }
        let scope = Scope {
            tab_id: tab_id.into(),
            kind: if worker {
                ReminderKind::Worker
            } else {
                ReminderKind::Owner
            },
            ..Default::default()
        };
        let snapshot = Snapshot {
            plan_id: plan.id.clone(),
            scope: scope.clone(),
            ..Default::default()
        };
        self.compose_plan_reminder(
            plan,
            &scope,
            &snapshot,
            &heartbeat_gate::cadence(&snapshot),
            true,
        )
        .text
    }

    fn send_due_plan_time_reminders(&self) {
        let cfg = self.configuration().orchestration;
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
    pub fn reconcile_time_reminder_eligibility(&self) {
        let plans = self.plan_store.list_full_reminder_candidates();
        let counter = &self.plan_turn_counter;
        counter.retain_time_reminders(&deadline_tabs(&plans, counter));
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
        preview: bool,
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
                for line in crate::service::cartographer::digest_lines(
                    plan,
                    self.configuration().orchestration.map.enabled,
                ) {
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
                    crate::service::plan::reminders::ORCHESTRATOR_STALLED_TEAM_REMINDER
                        .replace(
                            "{actionable}",
                            &orchestrator_actionable_task_ids(plan).join(", "),
                        )
                        .replace(
                            "{parked}",
                            &crate::service::plan::reminders::parked_later_group_summary(plan),
                        )
                } else {
                    String::new()
                };
                let mode_transition = snapshot.hard.contains(&HardWake::ModeChange);
                let plain = mode_transition && plan.mode != PlanMode::Execution;
                if plain {
                    text.clear();
                }
                text.push_str("\n\n");
                text.push_str(&decision.next_step_text());
                if !plain {
                    let resolution = self
                        .plan_store
                        .resolve_planner_tab(&plan.id, |pid| self.tab_probe(pid));
                    if let Some(prefix) =
                        crate::service::plan::peer_mandate::reminder_planner_prefix(&resolution)
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
                let digests = crate::service::orchestrator_digests::collect_digest_lines(
                    self,
                    plan,
                    if preview { None } else { Some(tab_id) },
                );
                let heartbeat = if plain {
                    String::new()
                } else {
                    self.plan_store.prepend_heartbeat_with_selected_learnings(
                        &plan.id,
                        "",
                        &digests,
                        decision.learning_ids.as_deref(),
                    )
                };
                let actions = crate::service::plan::reminders::composition::action_groups(plan);
                let parked = crate::service::plan::reminders::parked_later_group_summary(plan);
                let text = crate::service::plan::reminders::composition::compose(
                    plan,
                    &[&actions, &text, &parked, &heartbeat],
                );
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

    fn deliver_automated_plan_reminder(
        &self,
        tab_id: &str,
        text: &str,
        observer: Option<crate::service::messenger::DeliveryWriteObserver>,
    ) -> Result<bool, String> {
        let mut envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder("system");
        envelope.delivery_observer = observer;
        self.gateway_deliver(
            tab_id,
            envelope,
            text,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        )
        .map(|delivery| {
            if delivery.is_written() {
                self.record_kpi_event(
                    tab_id,
                    crate::agent::kpi_ledger::KpiEventKind::Nudge,
                    text.len(),
                );
            }
            delivery.is_written()
        })
        .map_err(|e| {
            log::warn!("[plan-reminder] deliver to {} failed: {}", tab_id, e);
            e
        })
    }

    fn notify_blocked(&self, _owner_tab: &str, notice: &BlockedNotice) {
        let tasks = if notice.task_ids.is_empty() {
            String::new()
        } else {
            format!("; tasks {}", notice.task_ids.join(", "))
        };
        let pause_proposal = if notice.pause_plan {
            " This blocker stops every actionable task; consider pausing the plan."
        } else {
            ""
        };
        self.log_system_notice(
            format!("Plan {} is blocked", notice.plan_id),
            Some(format!("{}{tasks}.{pause_proposal}", notice.summary)),
            Some("warning"),
        );
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

pub(crate) fn current_epoch_secs() -> u64 {
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
    if !active_execution(plan)
        || !matches!(
            task.status,
            TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Blocked
        )
    {
        return false;
    }
    !matches!(
        task_projection(plan, task).presentation,
        TaskPresentation::WaitingOnDependencies | TaskPresentation::ParkedForLaterGroup
    )
}

pub fn timed_plan_reminder_targets(plans: &[Plan]) -> Vec<TimedPlanReminderTarget> {
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

/// Tabs whose scheduler deadline survives a pass: timed targets plus owners of
/// complete plans still owed their one wrap-up.
fn deadline_tabs(plans: &[Plan], counter: &TurnCounter) -> HashSet<String> {
    timed_plan_reminder_targets(plans)
        .into_iter()
        .map(|target| target.tab_id)
        .chain(
            complete_wrapup_targets(plans)
                .into_iter()
                .filter(|(_, plan_id)| counter.complete_wrapup_due(plan_id))
                .map(|(tab_id, _)| tab_id),
        )
        .collect()
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
pub mod tests;
