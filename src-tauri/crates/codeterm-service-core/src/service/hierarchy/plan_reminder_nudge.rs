use crate::plan::PlanMode;
#[cfg(test)]
use crate::plan::{PlanStatus, TaskStatus};
use crate::service::hierarchy::heartbeat_gate::{
    self, Decision, Gate, HardWake, Observation, ReminderKind, Scope, Snapshot, TickFact,
};
use crate::service::plan::reminders::{NudgeKind, TurnCounter};
use crate::service::{ServiceContext, ServiceState};
use codeterm_plan::service_plan::{
    ReminderAdmissionRead as Plan, ReminderCandidateRead, ReminderCompositionRead,
    ReminderTargetRead,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod approval;
#[cfg(test)]
mod approval_tests;
#[cfg(test)]
mod consumer_load_tests;
mod projection;
#[cfg(test)]
mod target_parity_tests;

pub(crate) use projection::{project_for_tab, ProjectedHeartbeat, ProjectedTick};

use approval::{Emission, Recipient, ReminderHost, ReminderRequest, Transport, WorkerBindingRead};

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
    crate::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(plan_reminder_poll_secs(
                &svc.configuration().orchestration,
            )))
            .await;
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if svc.plan_store.claim_retention_day(now_ms) {
                let svc_for_retention = svc.clone();
                drop(crate::async_runtime::spawn_blocking(move || {
                    svc_for_retention.plan_store.prune_stale_plans(now_ms);
                }));
            }
            let svc_for_scan = svc.clone();
            if let Err(e) = crate::async_runtime::spawn_blocking(move || {
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

const AGENT_UNAVAILABLE: &str =
    "agent_unavailable: plan reminder target has no verified live agent session";

trait TimeReminderPorts {
    fn agent_live(&self, tab_id: &str) -> bool;
    fn deliver(
        &self,
        tab_id: &str,
        target: &PlanReminderTarget,
        kind: NudgeKind,
    ) -> Result<bool, String>;
    fn note(&self, tab_id: &str, target: &PlanReminderTarget, fact: TickFact);
    fn agent_lost(&self, tab_id: &str, target: &PlanReminderTarget);
}

fn effective_interval(
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
fn send_due_plan_time_reminders_via(
    plans: &[ReminderCandidateRead],
    counter: &TurnCounter,
    now: u64,
    interval_secs: u64,
    ports: &impl TimeReminderPorts,
) {
    use crate::service::plan::reminders::AgentLiveness;
    let targets = timed_plan_reminder_targets(plans);
    counter.retain_time_reminders(&deadline_tabs(plans, counter));
    let candidates = targets.len();
    let mut due = 0usize;
    for timed in targets {
        counter.record_time_tick(&timed.tab_id, now);
        let interval = effective_interval(plans, &timed.target, interval_secs);
        let is_due = counter.time_nudge_due(&timed.tab_id, now, interval);
        if !is_due {
            ports.note(&timed.tab_id, &timed.target, TickFact::NotDue);
            continue;
        }
        due += 1;
        counter.record_time_due(&timed.tab_id, now);
        let live = ports.agent_live(&timed.tab_id);
        match counter.observe_agent(&timed.tab_id, live) {
            Some(AgentLiveness::Lost) => {
                log::warn!(
                    "[plan-reminder] Tab {} (target {:?}) has no live agent; reminders pause until a session binds",
                    timed.tab_id,
                    timed.target
                );
                counter.record_time_failure(&timed.tab_id, AGENT_UNAVAILABLE);
                ports.agent_lost(&timed.tab_id, &timed.target);
            }
            Some(AgentLiveness::Restored) => log::info!(
                "[plan-reminder] Tab {} has a live agent again; reminders resume",
                timed.tab_id
            ),
            None => {}
        }
        if !live {
            ports.note(&timed.tab_id, &timed.target, TickFact::Unavailable);
            continue;
        }
        match ports.deliver(&timed.tab_id, &timed.target, timed.target.time_nudge_kind()) {
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

struct HostTick<'a, H, L> {
    host: &'a H,
    counter: &'a TurnCounter,
    now: u64,
    lost: L,
}

impl<H: ReminderHost, L: Fn(&str, &PlanReminderTarget)> TimeReminderPorts for HostTick<'_, H, L> {
    fn agent_live(&self, tab_id: &str) -> bool {
        ensure_routable(self.host, tab_id).is_ok()
    }

    fn deliver(
        &self,
        tab_id: &str,
        target: &PlanReminderTarget,
        kind: NudgeKind,
    ) -> Result<bool, String> {
        deliver_target(self.host, self.counter, tab_id, target, kind, self.now)
    }

    fn note(&self, tab_id: &str, target: &PlanReminderTarget, fact: TickFact) {
        record_target_tick(self.host, tab_id, target, self.now, fact);
    }

    fn agent_lost(&self, tab_id: &str, target: &PlanReminderTarget) {
        (self.lost)(tab_id, target);
    }
}

fn run_reminder_tick(
    host: &impl ReminderHost,
    plans: &[ReminderCandidateRead],
    counter: &TurnCounter,
    now: u64,
    interval_secs: u64,
    lost: impl Fn(&str, &PlanReminderTarget),
) {
    host.gate().retain(&approval::scope_keys(plans));
    let ports = HostTick {
        host,
        counter,
        now,
        lost,
    };
    send_due_plan_time_reminders_via(plans, counter, now, interval_secs, &ports);
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
            .plan(plan_id)?
            .map(|plan| plan.cooldown_key().map_err(|error| error.to_string()))
            .transpose()?
            .unwrap_or_else(|| {
                crate::service::supporter::criteria_hash(
                    format!("owner:{plan_id}:{kind:?}").as_bytes(),
                )
            }),
        PlanReminderTarget::Worker => {
            crate::service::supporter::criteria_hash(format!("worker:{tab_id}:{kind:?}").as_bytes())
        }
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
        Err(AGENT_UNAVAILABLE.into())
    }
}

fn blocked_notices(
    plan: &Plan,
    scope: &Scope,
    snapshot: &Snapshot,
) -> Result<Vec<BlockedNotice>, String> {
    let mut blockers = plan
        .blocker_facts(scope.task_id.as_deref())
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|blocker| {
            (
                blocker.identity,
                BlockerGroup {
                    summary: blocker.summary,
                    task_ids: blocker.task_ids,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
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
    let actionable: HashSet<_> = plan
        .actionable_task_ids()
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect();
    Ok(blockers
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
        .collect())
}

#[derive(Default)]
struct BlockerGroup {
    summary: String,
    task_ids: BTreeSet<String>,
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

    fn plan(&self, plan_id: &str) -> Result<Option<Plan>, String> {
        self.svc
            .plan_store
            .reminder_read_snapshot(heartbeat_gate::scrubbed)
            .and_then(|read| read.admission(plan_id))
            .map_err(|error| error.to_string())
    }

    fn worker_bindings(
        &self,
        tab: &str,
        known_plan: Option<&str>,
    ) -> Result<WorkerBindingRead, String> {
        let read = self
            .svc
            .plan_store
            .reminder_read_snapshot(heartbeat_gate::scrubbed)
            .map_err(|error| error.to_string())?;
        let mut bindings = read
            .worker_bindings(tab, known_plan)
            .map_err(|error| error.to_string())?;
        match (bindings.pop(), bindings.pop()) {
            (None, _) => Ok(WorkerBindingRead::Missing),
            (Some(_), Some(_)) => Ok(WorkerBindingRead::Ambiguous),
            (Some(binding), None) => {
                let plan = read
                    .admission(&binding.plan_id)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "reminder plan is missing from its snapshot".to_string())?;
                Ok(WorkerBindingRead::Bound {
                    plan: Box::new(plan),
                    task_id: binding.task_id,
                })
            }
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
    ) -> Result<(Snapshot, Observation), String> {
        heartbeat_gate::collect(self.svc, plan, tab_id, previous, now)
    }

    fn observe_worker(
        &self,
        plan: &Plan,
        scope: &Scope,
        previous: &Observation,
        _: u64,
    ) -> Result<(Snapshot, Observation), String> {
        let (mut snapshot, mut observed) =
            heartbeat_gate::collect_worker_scope(plan, scope, previous)?;
        snapshot.hard = heartbeat_gate::actionable_pending(&previous.pending_hard);
        let (_, owner_observation) =
            heartbeat_gate::collect_owner_messages(self.svc, &scope.tab_id, previous);
        if let Some(owner_observation) = owner_observation {
            observed
                .workers
                .insert(scope.tab_id.clone(), owner_observation);
        }
        observed.pending_hard = snapshot.hard.clone();
        Ok((snapshot, observed))
    }

    fn compose(
        &self,
        plan: &Plan,
        scope: &Scope,
        snapshot: &Snapshot,
        decision: &Decision,
    ) -> Result<Emission, String> {
        self.svc.compose_plan_reminder(
            &plan.composition().map_err(|error| error.to_string())?,
            scope,
            snapshot,
            decision,
            false,
        )
    }

    fn emit(
        &self,
        plan_id: &str,
        scope: &Scope,
        emission: &Emission,
        observer: Option<crate::service::messenger::DeliveryWriteObserver>,
    ) -> Result<bool, String> {
        let tab_id = scope.tab_id.as_str();
        crate::service::orchestrator_digests::apply_digest_rate_marks(
            self.svc,
            &emission.digest_rate_marks,
        );
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
        plan: &ReminderCompositionRead,
        tab_id: &str,
        worker: bool,
    ) -> Result<String, String> {
        if let Some(text) = self
            .ext::<std::sync::Arc<crate::service::hierarchy::heartbeat_gate::Gate>>()
            .last_body(tab_id, &plan.id)
        {
            return Ok(text);
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
        Ok(self
            .compose_plan_reminder(
                plan,
                &scope,
                &snapshot,
                &heartbeat_gate::cadence(&snapshot),
                true,
            )?
            .text)
    }

    fn send_due_plan_time_reminders(&self) {
        let cfg = self.configuration().orchestration;
        if !cfg.plan_reminders || cfg.nudge_interval_secs == 0 {
            self.plan_turn_counter.clear_time_reminders();
            return;
        }
        let plans = match self
            .plan_store
            .reminder_read_snapshot(heartbeat_gate::scrubbed)
            .and_then(|read| read.candidate_targets(None))
        {
            Ok(plans) => plans,
            Err(error) => {
                log::error!("[plan-reminder] candidate read failed: {error}");
                return;
            }
        };
        let host = LiveHost {
            svc: self,
            interval: cfg.nudge_interval_secs,
        };
        run_reminder_tick(
            &host,
            &plans,
            &self.plan_turn_counter,
            current_epoch_secs(),
            cfg.nudge_interval_secs,
            |tab_id, target| {
                if matches!(target, PlanReminderTarget::Worker) {
                    match self.worker_losses_for_tab(tab_id,crate::service::worker_loss::WorkerLossCause::AgentUnavailable) {
                        Ok(losses)=>self.report_worker_losses(losses),
                        Err(error)=>log::error!("[worker-lost] assignment read failed before reminder loss report: {error}"),
                    }
                }
            },
        );
    }

    /// Lifecycle mutations use this to clear only deadlines that became
    /// ineligible (paused, complete, parked/closed with no open work, or
    /// archived). Active execution owners keep their original deadline.
    pub fn reconcile_time_reminder_eligibility(&self) {
        let plans = match self
            .plan_store
            .reminder_read_snapshot(heartbeat_gate::scrubbed)
            .and_then(|read| read.candidate_targets(None))
        {
            Ok(plans) => plans,
            Err(error) => {
                log::error!("[plan-reminder] candidate read failed: {error}");
                return;
            }
        };
        let counter = &self.plan_turn_counter;
        counter.retain_time_reminders(&deadline_tabs(&plans, counter));
    }

    /// Host-authored reminder text; the model chose only whether it is useful.
    fn compose_plan_reminder(
        &self,
        plan: &ReminderCompositionRead,
        scope: &Scope,
        snapshot: &Snapshot,
        decision: &Decision,
        preview: bool,
    ) -> Result<Emission, String> {
        let tab_id = scope.tab_id.as_str();
        Ok(match scope.kind {
            ReminderKind::Worker => {
                let mut text = plan
                    .compose_worker(
                        tab_id,
                        &crate::service::plan::reminders::worker_nudge_text(),
                    )
                    .map_err(|error| error.to_string())?;
                // The live tick runs on a blocking worker; a popup preview may not.
                if !preview {
                    if let Some(step) = self.tab_todo_step_line_blocking(tab_id) {
                        text.push_str("\n\n");
                        text.push_str(&step);
                    }
                }
                Emission {
                    text,
                    transport: Transport::Automated,
                    digest_rate_marks: Vec::new(),
                }
            }
            ReminderKind::Owner => {
                let mut text = String::new();
                let mode_transition = snapshot.hard.contains(&HardWake::ModeChange);
                let plain = mode_transition && plan.mode != PlanMode::Execution;
                if plain {
                    text.clear();
                }
                text.push_str("\n\n");
                text.push_str(&decision.next_step_text());
                if !plain {
                    let resolution = plan
                        .planner_resolution(|pid| self.tab_probe(pid))
                        .map_err(|error| error.to_string())?;
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
                let inventory = crate::service::orchestrator_digests::snapshot_notice_inventory(
                    self,
                    &plan.id,
                    plan.team_id.as_deref(),
                    plan.owner_tab_id.as_deref(),
                );
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let input = plan
                    .digest_input(now, &inventory.targets())
                    .map_err(|error| error.to_string())?;
                let digests = crate::service::orchestrator_digests::prepare_digest_lines_for_input(
                    self,
                    &input,
                    &inventory,
                    if preview { None } else { Some(tab_id) },
                );
                let text = plan
                    .compose_owner(
                        &text,
                        &digests.lines,
                        decision.learning_ids.as_deref(),
                        plain,
                        now,
                    )
                    .map_err(|error| error.to_string())?;
                Emission {
                    text,
                    digest_rate_marks: digests.rate_marks,
                    transport: Transport::Heartbeat {
                        learning_ids: decision.learning_ids.clone(),
                        plain,
                        mode_transition,
                    },
                }
            }
        })
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

pub(crate) fn deliver_tab_todo_recitation(
    svc: &ServiceContext,
    tab_id: &str,
    text: &str,
) -> Result<bool, String> {
    svc.gateway_deliver(
        tab_id,
        crate::service::messenger::MessageEnvelope::codeterm_reminder("checklist"),
        text,
        crate::service::message_gateway::DeliveryClass::SystemNotice,
    )
    .map(|delivery| delivery.is_written())
}

#[cfg(test)]
fn plan_reminder_state_key(plan: &Plan) -> String {
    plan.cooldown_key().expect("fixture cooldown key")
}

impl PlanReminderTarget {
    fn time_nudge_kind(&self) -> NudgeKind {
        match self {
            Self::Owner { .. } => NudgeKind::Orchestrator,
            Self::Worker => NudgeKind::Worker,
        }
    }

    fn request(&self, tab_id: &str, _kind: NudgeKind) -> ReminderRequest {
        match self {
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

fn owner_reminder_eligible(plan: &Plan) -> bool {
    plan.control_loop.owner_eligible(
        &plan.status,
        &plan.mode,
        plan.complete(),
        plan.owner_open_work,
    )
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

fn non_empty(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    (!value.is_empty()).then_some(value)
}

fn deadline_tabs(plans: &[ReminderCandidateRead], _counter: &TurnCounter) -> HashSet<String> {
    timed_plan_reminder_targets(plans)
        .into_iter()
        .map(|target| target.tab_id)
        .collect()
}

#[cfg(test)]
pub mod tests;
