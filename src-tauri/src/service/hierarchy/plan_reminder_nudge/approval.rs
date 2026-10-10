use std::collections::HashSet;

use crate::multiplexer::types::Tab;
use crate::plan::{Plan, PlanMode, PlanStatus};
use crate::service::hierarchy::heartbeat_gate::{
    self, Claim, Decision, Gate, Observation, ReminderKind, Scope, Snapshot, Supporter,
};
use codeterm_plan::service_plan::state::orchestrator_actionable_task_ids;

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
    fn supporter(&self) -> Supporter<'_>;
    fn interval(&self) -> u64;
    fn plan(&self, plan_id: &str) -> Option<Plan>;
    fn with_plans<R>(&self, read: impl FnOnce(&[Plan]) -> R) -> R;
    fn recipient(&self, tab_id: &str) -> Option<Recipient>;
    fn observe_owner(
        &self,
        plan: &Plan,
        tab_id: &str,
        previous: &Observation,
        now: u64,
    ) -> (Snapshot, Observation);
    fn observe_worker(
        &self,
        plan: &Plan,
        scope: &Scope,
        previous: &Observation,
        now: u64,
    ) -> (Snapshot, Observation);
    fn compose(
        &self,
        plan: &Plan,
        scope: &Scope,
        snapshot: &Snapshot,
        decision: &Decision,
    ) -> Emission;
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
pub(crate) fn scope_keys(plans: &[Plan]) -> HashSet<String> {
    plans
        .iter()
        .flat_map(|plan| {
            non_empty(plan.owner.tab_id.as_deref())
                .into_iter()
                .chain(
                    plan.tasks
                        .iter()
                        .filter_map(|task| non_empty(task.assigned_tab_id.as_deref())),
                )
                .map(|tab_id| scope_key(&plan.id, tab_id))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The only path to an automatic plan reminder: exact scope, connected Choice
/// adapter, one approving verdict, fresh revalidation, then one emitter call.
pub(crate) fn admit(
    host: &impl ReminderHost,
    request: &ReminderRequest,
    now: u64,
) -> ReminderOutcome {
    let (plan, scope) = match resolve(host, request, None) {
        Ok(found) => found,
        Err(reason) => return ReminderOutcome::Suppressed(reason),
    };
    let gate = host.gate();
    let claim = match gate.claim(&scope_key(&plan.id, &scope.tab_id)) {
        Ok(claim) => claim,
        Err(refusal) => return ReminderOutcome::Suppressed(refusal.reason()),
    };
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
    match outcome {
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
    let supporter = host.supporter();
    let soft_cooldown = if scope.kind == ReminderKind::Worker {
        0
    } else {
        host.interval()
    };
    let (Some(model), Some(adapter)) = (supporter.connected(&scope.tab_id), supporter.generation())
    else {
        return suppress(gate, claim, "no_adapter");
    };
    let key = claim.key();
    let previous = gate.observation(key);
    let (mut snapshot, observed) = observe(host, &plan, &scope, &previous, now);
    gate.observed(key, observed);
    snapshot.scope = Scope {
        adapter: adapter.clone(),
        criteria: heartbeat_gate::criteria_version(),
        ..scope.clone()
    };
    let decided = gate.decide_with_interval(
        key,
        &snapshot,
        now,
        Some(model.as_ref()),
        supporter.deadline(),
        soft_cooldown,
    );
    let Ok(mut decision) = decided else {
        return suppress(gate, claim, "gate_unavailable");
    };
    decision.adapter = Some(model.id().to_string());
    decision.model = model.model_id();
    heartbeat_gate::journal(&supporter.runtime, &decision, &plan.id, now);
    let current = |plan_id: &str| match resolve(host, request, Some(plan_id)) {
        Ok((plan, fresh)) if fresh == scope && connected_as(host, &supporter, &adapter) => {
            Some(plan)
        }
        _ => None,
    };
    if decision.tick_label == "blocked" {
        if decision.reason != "model_answer" || decision.decided_by == "cache" {
            return suppress(gate, claim, decision.reason);
        }
        let Some(plan) = current(&plan.id) else {
            return suppress(gate, claim, "stale_scope");
        };
        let (mut fresh, _) = observe(host, &plan, &scope, &previous, now);
        fresh.scope = snapshot.scope.clone();
        if fresh.hash() != decision.snapshot_hash {
            return suppress(gate, claim, "stale_observation");
        }
        if let Some(owner_tab) = non_empty(plan.owner.tab_id.as_deref()) {
            for notice in super::blocked_notices(&plan, &scope, &fresh) {
                if gate.claim_blocker_notice(&notice.event_key) {
                    host.notify_blocked(owner_tab, &notice);
                }
            }
        }
        return suppress(gate, claim, "recipient_blocked");
    }
    if !decision.wake {
        return suppress(gate, claim, decision.reason);
    }
    let Some(plan) = current(&plan.id) else {
        return suppress(gate, claim, "stale_scope");
    };
    // The approved identity must still describe the plan the text is built from.
    let (mut fresh, _) = observe(host, &plan, &scope, &previous, now);
    fresh.scope = snapshot.scope.clone();
    if fresh.hash() != decision.snapshot_hash {
        return suppress(gate, claim, "stale_observation");
    }
    if scope.kind == ReminderKind::Worker
        && scope.task_id.as_deref().is_some_and(|task_id| {
            plan.tasks
                .iter()
                .find(|task| task.id == task_id)
                .is_some_and(|task| task.status == crate::plan::TaskStatus::Blocked)
        })
    {
        return suppress(gate, claim, "recipient_not_actionable");
    }
    let emission = host.compose(&plan, &scope, &snapshot, &decision);
    // Re-observation and compose can take IO time; the last check is bound to the handoff.
    if current(&plan.id).is_none() || !gate.attempt(claim, &decision) {
        return suppress(gate, claim, "stale_scope");
    }
    match host.emit(
        &plan.id,
        &scope,
        &emission,
        request.delivery_observer.clone(),
    ) {
        Ok(true) => {
            gate.delivered(key, &snapshot, &decision, now);
            ReminderOutcome::Accepted
        }
        Ok(false) => ReminderOutcome::Pending,
        Err(error) => {
            gate.failed(key);
            ReminderOutcome::Failed(error)
        }
    }
}

/// The connection epoch and adapter fingerprint still equal those of the
/// approving decision; a fresh Supporter view is read on every call.
fn connected_as(host: &impl ReminderHost, approved: &Supporter<'_>, adapter: &str) -> bool {
    let current = host.supporter();
    current.epoch == approved.epoch && current.generation().as_deref() == Some(adapter)
}

fn observe(
    host: &impl ReminderHost,
    plan: &Plan,
    scope: &Scope,
    previous: &Observation,
    now: u64,
) -> (Snapshot, Observation) {
    match scope.kind {
        ReminderKind::Worker => host.observe_worker(plan, scope, previous, now),
        _ => host.observe_owner(plan, &scope.tab_id, previous, now),
    }
}

fn suppress(gate: &Gate, claim: &Claim<'_>, reason: &'static str) -> ReminderOutcome {
    gate.suppressed(claim.key(), reason);
    ReminderOutcome::Suppressed(reason)
}

fn resolve(
    host: &impl ReminderHost,
    request: &ReminderRequest,
    known_plan: Option<&str>,
) -> Result<(Plan, Scope), &'static str> {
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
    let plan = request
        .plan_id
        .as_deref()
        .and_then(|plan_id| host.plan(plan_id))
        .ok_or("plan_missing")?;
    if non_empty(plan.owner.tab_id.as_deref()) != Some(request.tab_id.as_str())
        || !owner_eligible(&plan, request.kind)
    {
        return Err("ineligible");
    }
    let binding = heartbeat_gate::binding_digest(&recipient.tab, None).ok_or("binding_missing")?;
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
) -> Result<(Plan, Scope), &'static str> {
    let tab_id = request.tab_id.as_str();
    let mut matches = match known_plan {
        Some(plan_id) => {
            worker_matches(&host.plan(plan_id).into_iter().collect::<Vec<_>>(), tab_id)
        }
        None => host.with_plans(|plans| worker_matches(plans, tab_id)),
    };
    let (plan, task_id) = match (matches.pop(), matches.pop()) {
        (Some(found), None) => found,
        (None, _) => return Err("binding_missing"),
        _ => return Err("binding_ambiguous"),
    };
    let task = plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or("binding_missing")?;
    let mut open = task
        .execution_attempts
        .iter()
        .filter(|attempt| attempt.closed_at.is_none() && attempt.executor_tab_id == tab_id);
    let attempt = match (open.next(), open.next()) {
        (Some(attempt), None) => attempt,
        (None, _) => return Err("binding_missing"),
        _ => return Err("binding_ambiguous"),
    };
    let binding =
        heartbeat_gate::binding_digest(&recipient.tab, Some(attempt)).ok_or("binding_missing")?;
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

fn worker_matches(plans: &[Plan], tab_id: &str) -> Vec<(Plan, String)> {
    plans
        .iter()
        .flat_map(|plan| {
            plan.tasks
                .iter()
                .filter(move |task| {
                    non_empty(task.assigned_tab_id.as_deref()) == Some(tab_id)
                        && super::worker_reminder_eligible(plan, task)
                })
                .map(move |task| (plan, task.id.clone()))
        })
        .take(2)
        .map(|(plan, task_id)| (plan.clone(), task_id))
        .collect()
}

fn owner_eligible(plan: &Plan, kind: ReminderKind) -> bool {
    let active = plan.status == PlanStatus::Active;
    match kind {
        ReminderKind::Owner => super::owner_reminder_eligible(plan),
        ReminderKind::ModeChange => active && plan.mode != PlanMode::Execution,
        ReminderKind::StalledTeam => {
            active
                && plan.mode == PlanMode::Execution
                && !orchestrator_actionable_task_ids(plan).is_empty()
        }
        ReminderKind::CompleteWrapup => {
            crate::service::plan::complete_wrapup::needs_complete_wrapup(plan)
                && !crate::service::plan::pause_operation::plan_notices_suppressed(plan)
        }
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
