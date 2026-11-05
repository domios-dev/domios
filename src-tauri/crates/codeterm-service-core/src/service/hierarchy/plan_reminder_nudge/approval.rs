use std::collections::HashSet;

use crate::multiplexer::types::Tab;
use crate::service::hierarchy::heartbeat_gate::{
    self, Claim, Decision, Gate, Observation, ReminderKind, Scope, Snapshot, TickFact,
};
use codeterm_plan::service_plan::{ReminderAdmissionRead as Plan, ReminderCandidateRead};

use super::non_empty;
use super::BlockedNotice;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReminderRequest {
    pub(crate) kind: ReminderKind,
    pub(crate) plan_id: Option<String>,
    pub(crate) tab_id: String,
    pub(crate) delivery_observer: Option<crate::service::messenger::DeliveryWriteObserver>,
}

impl ReminderRequest {
    pub(crate) fn owner(plan_id: &str, tab_id: &str, kind: ReminderKind) -> Self {
        Self {
            kind,
            plan_id: Some(plan_id.into()),
            tab_id: tab_id.into(),
            delivery_observer: None,
        }
    }

    pub(crate) fn worker(tab_id: &str) -> Self {
        Self {
            kind: ReminderKind::Worker,
            plan_id: None,
            tab_id: tab_id.into(),
            delivery_observer: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReminderOutcome {
    Suppressed(&'static str),
    Accepted,
    Pending,
    Failed(String),
}

impl ReminderOutcome {
    /// Producer contract: only emitter acceptance is a delivery.
    pub(crate) fn into_delivery(self) -> Result<bool, String> {
        match self {
            Self::Accepted => Ok(true),
            Self::Suppressed(_) | Self::Pending => Ok(false),
            Self::Failed(error) => Err(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Transport {
    Heartbeat {
        learning_ids: Option<Vec<String>>,
        plain: bool,
        mode_transition: bool,
    },
    Automated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Emission {
    pub(crate) text: String,
    pub(crate) transport: Transport,
    pub(crate) digest_rate_marks: Vec<crate::service::orchestrator_digests::DigestRateMark>,
}

/// A live tab together with the workspace whose membership currently holds it,
/// read under one multiplexer view.
pub(crate) struct Recipient {
    pub(crate) tab: Tab,
    pub(crate) workspace_id: Option<String>,
}

/// What admission reads or writes outside the gate. The daemon and the tests
/// each supply one, so the approval policy itself has a single implementation.
pub(crate) trait ReminderHost {
    fn gate(&self) -> &Gate;
    fn journal_runtime(&self) -> std::sync::Arc<crate::service::supporter::SupporterRuntime>;
    fn interval(&self) -> u64;
    fn plan(&self, plan_id: &str) -> Result<Option<Plan>, String>;
    fn worker_bindings(
        &self,
        tab: &str,
        known_plan: Option<&str>,
    ) -> Result<WorkerBindingRead, String>;
    fn recipient(&self, tab_id: &str) -> Option<Recipient>;
    fn observe_owner(
        &self,
        plan: &Plan,
        tab_id: &str,
        previous: &Observation,
        now: u64,
    ) -> Result<(Snapshot, Observation), String>;
    fn observe_worker(
        &self,
        plan: &Plan,
        scope: &Scope,
        previous: &Observation,
        now: u64,
    ) -> Result<(Snapshot, Observation), String>;
    fn compose(
        &self,
        plan: &Plan,
        scope: &Scope,
        snapshot: &Snapshot,
        decision: &Decision,
    ) -> Result<Emission, String>;
    fn emit(
        &self,
        plan_id: &str,
        scope: &Scope,
        emission: &Emission,
        observer: Option<crate::service::messenger::DeliveryWriteObserver>,
    ) -> Result<bool, String>;
    fn notify_blocked(&self, owner_tab: &str, notice: &BlockedNotice);
}

pub(crate) fn scope_key(plan_id: &str, tab_id: &str) -> String {
    format!("{plan_id}:{tab_id}")
}

/// Every scope a current candidate plan can produce; the gate keeps no others.
pub(crate) fn scope_keys(plans: &[ReminderCandidateRead]) -> HashSet<String> {
    plans
        .iter()
        .flat_map(|plan| {
            non_empty(plan.owner_tab_id.as_deref())
                .into_iter()
                .chain(
                    plan.retained_scope_tabs
                        .as_ref()
                        .into_iter()
                        .flatten()
                        .filter_map(|tab| non_empty(Some(tab))),
                )
                .map(|tab_id| scope_key(&plan.id, tab_id))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Automatic reminders require a rule verdict and fresh scope revalidation.
pub(crate) fn admit(
    host: &impl ReminderHost,
    request: &ReminderRequest,
    now: u64,
) -> ReminderOutcome {
    let (plan, scope) = match resolve(host, request, None) {
        Ok(found) => found,
        Err(ResolveError::Suppressed(reason)) => {
            note_request_tick(
                host.gate(),
                request,
                now,
                TickFact::Suppressed {
                    reason: reason.into(),
                },
            );
            return ReminderOutcome::Suppressed(reason);
        }
        Err(ResolveError::Read(error)) => {
            note_request_tick(host.gate(), request, now, TickFact::Failed);
            return ReminderOutcome::Failed(error);
        }
    };
    let gate = host.gate();
    let claim = match gate.claim(&scope_key(&plan.id, &scope.tab_id)) {
        Ok(claim) => claim,
        Err(refusal) => {
            let reason = refusal.reason();
            note_request_tick(
                gate,
                request,
                now,
                TickFact::Suppressed {
                    reason: reason.into(),
                },
            );
            return ReminderOutcome::Suppressed(reason);
        }
    };
    let last_reason = gate.telemetry(claim.key()).last_reason;
    let outcome = admit_claimed(host, request, &claim, plan, scope, now);
    let phases = gate.telemetry(claim.key());
    let line = format!(
        "[plan-reminder] {} reminder for scope {}: {outcome:?}; suppressed={} attempted={} accepted={} failed={} last={:?}",
        request.kind.name(),
        claim.key(),
        phases.suppressed,
        phases.attempted,
        phases.accepted,
        phases.failed,
        phases.last_reason
    );
    // A due reminder must never vanish unseen: each new suppression reason reaches the release log.
    match outcome {
        ReminderOutcome::Suppressed(reason) if last_reason != Some(reason) => log::info!("{line}"),
        ReminderOutcome::Suppressed(_) => log::debug!("{line}"),
        ReminderOutcome::Accepted => log::info!("{line}"),
        ReminderOutcome::Pending => log::warn!("{line}"),
        ReminderOutcome::Failed(_) => log::warn!("{line}"),
    }
    outcome
}

fn admit_claimed(
    host: &impl ReminderHost,
    request: &ReminderRequest,
    claim: &Claim<'_>,
    plan: Plan,
    scope: Scope,
    now: u64,
) -> ReminderOutcome {
    let gate = host.gate();
    let key = claim.key();
    let previous = gate.observation(key);
    let (mut snapshot, observed) = match observe(host, &plan, &scope, &previous, now) {
        Ok(observed) => observed,
        Err(error) => return failed(host, claim, request, now, error),
    };
    gate.observed(key, observed);
    snapshot.scope = Scope {
        criteria: heartbeat_gate::criteria_version(),
        ..scope.clone()
    };
    if scope.kind == ReminderKind::Worker
        && scope.task_id.as_deref().is_some_and(|id| {
            plan.tasks
                .iter()
                .any(|task| task.id == id && task.status == crate::plan::TaskStatus::Blocked)
        })
    {
        if let Some(owner) = non_empty(plan.owner_tab_id.as_deref()) {
            let notices = match super::blocked_notices(&plan, &scope, &snapshot) {
                Ok(notices) => notices,
                Err(error) => return failed(host, claim, request, now, error),
            };
            for notice in notices {
                if gate.claim_blocker_notice(&notice.event_key) {
                    host.notify_blocked(owner, &notice);
                }
            }
        }
        return suppress(gate, claim, request, now, "recipient_not_actionable");
    }
    let decision = gate
        .decide_with_interval(key, &snapshot, now, host.interval())
        .ok();
    let Some(decision) = decision else {
        return suppress(gate, claim, request, now, "gate_unavailable");
    };
    if !matches!(decision.reason, "same_snapshot" | "healthy_snapshot") {
        heartbeat_gate::journal(&host.journal_runtime(), &decision, &plan.id, now);
    }
    let current = |plan_id: &str| match resolve(host, request, Some(plan_id)) {
        Ok((plan, fresh)) if fresh == scope => Ok(Some(plan)),
        Err(ResolveError::Read(error)) => Err(error),
        _ => Ok(None),
    };
    if !decision.wake {
        return suppress(gate, claim, request, now, decision.reason);
    }
    let plan_id = plan.id.clone();
    drop(plan);
    let plan = match current(&plan_id) {
        Ok(Some(plan)) => plan,
        Ok(None) => return suppress(gate, claim, request, now, "stale_scope"),
        Err(error) => return failed(host, claim, request, now, error),
    };
    // The approved identity must still describe the plan the text is built from.
    let (mut fresh, _) = match observe(host, &plan, &scope, &previous, now) {
        Ok(fresh) => fresh,
        Err(error) => return failed(host, claim, request, now, error),
    };
    fresh.scope = snapshot.scope.clone();
    if fresh.hash() != decision.snapshot_hash {
        return suppress(gate, claim, request, now, "stale_observation");
    }
    if scope.kind == ReminderKind::Worker
        && scope.task_id.as_deref().is_some_and(|task_id| {
            plan.tasks
                .iter()
                .find(|task| task.id == task_id)
                .is_some_and(|task| task.status == crate::plan::TaskStatus::Blocked)
        })
    {
        return suppress(gate, claim, request, now, "recipient_not_actionable");
    }
    let emission = match host.compose(&plan, &scope, &snapshot, &decision) {
        Ok(emission) => emission,
        Err(error) => {
            gate.failed(key);
            note_request_tick(gate, request, now, TickFact::Failed);
            return ReminderOutcome::Failed(error);
        }
    };
    // Re-observation and compose can take IO time; the last check is bound to the handoff.
    let current_scope = match current(&plan.id) {
        Ok(plan) => plan.is_some(),
        Err(error) => return failed(host, claim, request, now, error),
    };
    if !current_scope || !gate.attempt(claim, &decision) {
        return suppress(gate, claim, request, now, "stale_scope");
    }
    drop(plan);
    match host.emit(
        &plan_id,
        &scope,
        &emission,
        request.delivery_observer.clone(),
    ) {
        Ok(true) => {
            gate.delivered(key, &snapshot, &decision, now);
            note_request_tick(
                gate,
                request,
                now,
                TickFact::Accepted {
                    decided_by: decision.decided_by.clone(),
                },
            );
            ReminderOutcome::Accepted
        }
        Ok(false) => {
            note_request_tick(gate, request, now, TickFact::Pending);
            ReminderOutcome::Pending
        }
        Err(error) => {
            gate.failed(key);
            note_request_tick(gate, request, now, TickFact::Failed);
            ReminderOutcome::Failed(error)
        }
    }
}

fn observe(
    host: &impl ReminderHost,
    plan: &Plan,
    scope: &Scope,
    previous: &Observation,
    now: u64,
) -> Result<(Snapshot, Observation), String> {
    let (mut snapshot, observation) = match scope.kind {
        ReminderKind::Worker => host.observe_worker(plan, scope, previous, now),
        _ => host.observe_owner(plan, &scope.tab_id, previous, now),
    }?;
    snapshot.actionable_tasks = if scope.kind == ReminderKind::Worker {
        scope
            .task_id
            .iter()
            .filter(|id| {
                plan.tasks
                    .iter()
                    .any(|task| &task.id == *id && task.status != crate::plan::TaskStatus::Blocked)
            })
            .cloned()
            .collect()
    } else {
        plan.actionable_task_ids()
            .map_err(|error| error.to_string())?
    };
    Ok((snapshot, observation))
}

fn note_request_tick(gate: &Gate, request: &ReminderRequest, now: u64, fact: TickFact) {
    gate.record_tick(heartbeat_gate::delivery_tick(
        &request.tab_id,
        request.plan_id.as_deref().unwrap_or(""),
        now,
        fact,
    ));
}

fn suppress(
    gate: &Gate,
    claim: &Claim<'_>,
    request: &ReminderRequest,
    now: u64,
    reason: &'static str,
) -> ReminderOutcome {
    gate.suppressed(claim.key(), reason);
    note_request_tick(
        gate,
        request,
        now,
        TickFact::Suppressed {
            reason: reason.into(),
        },
    );
    ReminderOutcome::Suppressed(reason)
}

fn resolve(
    host: &impl ReminderHost,
    request: &ReminderRequest,
    known_plan: Option<&str>,
) -> Result<(Plan, Scope), ResolveError> {
    let recipient = host
        .recipient(&request.tab_id)
        .filter(|recipient| {
            crate::service::plan_reconcile::tab_has_live_agent(&recipient.tab)
                && recipient.workspace_id.is_some()
        })
        .ok_or("recipient_unavailable")?;
    if request.kind == ReminderKind::Worker {
        return resolve_worker(host, &recipient, request, known_plan);
    }
    let plan_id = request.plan_id.as_deref().ok_or("plan_missing")?;
    let plan = host
        .plan(plan_id)
        .map_err(ResolveError::Read)?
        .ok_or("plan_missing")?;
    if non_empty(plan.owner_tab_id.as_deref()) != Some(request.tab_id.as_str())
        || !owner_eligible(&plan, request.kind)
    {
        return Err("ineligible".into());
    }
    let binding = heartbeat_gate::binding_digest(&recipient.tab, None).ok_or("binding_missing")?;
    let plan = plan
        .with_gate_facts()
        .map_err(|error| ResolveError::Read(error.to_string()))?;
    let lifecycle = format!("{:?}:{:?}", plan.status, plan.mode);
    let scope = scope(
        &plan,
        &recipient,
        request,
        "orchestrator",
        binding,
        None,
        lifecycle,
    )?;
    Ok((plan, scope))
}

/// A Worker reminder belongs to exactly one eligible task and one open
/// attempt bound to the tab's current session; anything else suppresses.
fn resolve_worker(
    host: &impl ReminderHost,
    recipient: &Recipient,
    request: &ReminderRequest,
    known_plan: Option<&str>,
) -> Result<(Plan, Scope), ResolveError> {
    let tab_id = request.tab_id.as_str();
    let (plan, task_id) = match host
        .worker_bindings(tab_id, known_plan)
        .map_err(ResolveError::Read)?
    {
        WorkerBindingRead::Bound { plan, task_id } => (*plan, task_id),
        WorkerBindingRead::Missing => return Err("binding_missing".into()),
        WorkerBindingRead::Ambiguous => return Err("binding_ambiguous".into()),
    };
    let plan = plan
        .with_worker_gate_facts(&task_id)
        .map_err(|error| ResolveError::Read(error.to_string()))?;
    let task = plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or("binding_missing")?;
    let mut open = task
        .attempts
        .iter()
        .filter(|attempt| !attempt.closed && attempt.executor_tab_id == tab_id);
    let attempt = match (open.next(), open.next()) {
        (Some(attempt), None) => attempt,
        (None, _) => return Err("binding_missing".into()),
        _ => return Err("binding_ambiguous".into()),
    };
    let binding = heartbeat_gate::binding_digest_fields(
        &recipient.tab,
        attempt.session_id.as_deref(),
        attempt.session_binding_nonce.as_deref(),
    )
    .ok_or("binding_missing")?;
    let lifecycle = format!("{:?}:{:?}:{:?}", plan.status, plan.mode, task.status);
    let attempt_scope = Some((task.id.as_str(), attempt.id.as_str()));
    let scope = scope(
        &plan,
        recipient,
        request,
        "worker",
        binding,
        attempt_scope,
        lifecycle,
    )?;
    Ok((plan, scope))
}

fn owner_eligible(plan: &Plan, kind: ReminderKind) -> bool {
    match kind {
        ReminderKind::Owner => super::owner_reminder_eligible(plan),
        ReminderKind::Worker => false,
    }
}

/// The authenticated recipient's own Team, workspace and role are part of the
/// scope; a recipient outside the plan's Team never receives its reminder.
fn scope(
    plan: &Plan,
    recipient: &Recipient,
    request: &ReminderRequest,
    role: &str,
    binding: String,
    attempt: Option<(&str, &str)>,
    lifecycle: String,
) -> Result<Scope, &'static str> {
    let tab = &recipient.tab;
    if plan.team_id.is_some() && tab.team_id != plan.team_id {
        return Err("recipient_scope_mismatch");
    }
    Ok(Scope {
        kind: request.kind,
        role: role.into(),
        workspace: plan.project_path.clone(),
        team_id: plan.team_id.clone(),
        tab_id: request.tab_id.clone(),
        recipient_team: tab.team_id.clone(),
        recipient_workspace: recipient.workspace_id.clone(),
        recipient_role: format!("{:?}:{}", tab.role, tab.role_kind_id().unwrap_or("")),
        binding,
        task_id: attempt.map(|(task, _)| task.into()),
        attempt_id: attempt.map(|(_, attempt)| attempt.into()),
        lifecycle,
        ..Default::default()
    })
}

#[derive(Debug)]
enum ResolveError {
    Suppressed(&'static str),
    Read(String),
}

impl From<&'static str> for ResolveError {
    fn from(reason: &'static str) -> Self {
        Self::Suppressed(reason)
    }
}

fn failed(
    host: &impl ReminderHost,
    claim: &Claim<'_>,
    request: &ReminderRequest,
    now: u64,
    error: String,
) -> ReminderOutcome {
    host.gate().failed(claim.key());
    note_request_tick(host.gate(), request, now, TickFact::Failed);
    ReminderOutcome::Failed(error)
}

pub(crate) enum WorkerBindingRead {
    Missing,
    Ambiguous,
    Bound { plan: Box<Plan>, task_id: String },
}
