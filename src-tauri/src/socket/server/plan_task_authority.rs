use serde_json::Value;

use super::{plan_refs, JsonRpcError};
use crate::service::plan::interruption::TaskMutationAuthority;
use crate::service::plan::PlanError;
use crate::service::ServiceState;

pub(super) fn authorize_draft_mutation(
    svc: &ServiceState,
    plan_id: &str,
    params: Option<&Value>,
) -> Result<(), JsonRpcError> {
    svc.classify_planning_caller(
        plan_id,
        &crate::service::AuthenticatedPlanCaller {
            tab_id: params
                .and_then(|value| value.get("caller_tab_id"))
                .and_then(Value::as_str)
                .map(str::to_string),
            session_id: params
                .and_then(|value| value.get("session_id"))
                .and_then(Value::as_str)
                .map(str::to_string),
            binding_nonce: params
                .and_then(|value| value.get("session_binding_nonce"))
                .and_then(Value::as_str)
                .map(str::to_string),
        },
    )
    .map(|_| ())
    .map_err(|error| JsonRpcError {
        code: -32003,
        message: error.to_string(),
        data: Some(serde_json::json!({ "type": error.diagnostic_type() })),
    })
}

/// Recovery verbs the live Plan owner may drive against a tab-gone task. A stale
/// epoch is the CONDITION recovery exists for, so it must not be what blocks it.
pub(super) const RECOVERY_METHODS: &[&str] = &[
    "plan.task.update",
    "plan.task.assign",
    "plan.task.dispatch",
    "plan.task.respawn",
    "plan.task.close",
];

/// Verbs where the live Plan owner is an authority over a task it does not hold
/// itself. Closed on purpose: a verb newly routed through this seam to assert the
/// CALLER's own task binding inherits neither the shortcut nor recovery.
pub(super) const PLAN_OWNER_AUTHORITY_METHODS: &[&str] = &[
    "task.take",
    "task.drop",
    "plan.task.update",
    "plan.task.attach",
    "plan.task.close",
    "plan.task.assign",
    "plan.task.dispatch",
    "plan.task.respawn",
    "plan.task.compete",
    "plan.task.judge",
    "plan.task.rm",
    "plan.task.link",
    "plan.task.relink",
];

pub(super) fn is_recovery_method(method: &str) -> bool {
    RECOVERY_METHODS.contains(&method)
}

pub(super) fn is_plan_owner_authority_method(method: &str) -> bool {
    PLAN_OWNER_AUTHORITY_METHODS.contains(&method)
}

/// Verbs a bound Planner may drive on a task it does not own, because they carry
/// nothing but structure. Closed on purpose and deliberately NOT the recovery or
/// plan-owner lists: status, assignment, dispatch and approval are absent, so a
/// verb added to those lists inherits no Planner authority from this one.
pub(super) const PLANNER_STRUCTURE_METHODS: &[&str] = &["plan.task.update"];

/// A bound Planner editing task structure. Its authority over structure is
/// durable for the plan's whole life, so it must not lapse the moment a task is
/// dispatched once and its owner epoch leaves zero — that was the accidental
/// grant this seam used to give through the `owner_epoch == 0` short-circuit.
///
/// Bounded by the SAME field list the update handler enforces, so widening here
/// cannot widen what a Planner may write.
fn planner_may_edit_structure(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
    plan_id: &str,
) -> bool {
    if !PLANNER_STRUCTURE_METHODS.contains(&method) {
        return false;
    }
    let body = params.as_ref().unwrap_or(&Value::Null);
    if crate::service::planner_forbidden_task_field(body).is_some() {
        return false;
    }
    matches!(
        svc.classify_planning_caller(plan_id, &plan_caller(params)),
        Ok(crate::plan::PlanningAuthor::Planner { .. })
    )
}

/// Stable discriminants for task-authority refusals, carried as `data.type`.
/// Gates and tests key off these so the prose stays free to change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TaskAuthorityRefusal {
    OwnerEpochStale,
    CallerBindingStale,
    OwnerGoneVerbNotRecoverable,
    CallerNotTaskOwner,
}

impl TaskAuthorityRefusal {
    pub(super) fn discriminant(self) -> &'static str {
        match self {
            Self::OwnerEpochStale => "task_owner_epoch_stale",
            Self::CallerBindingStale => "task_mutation_binding_stale",
            Self::OwnerGoneVerbNotRecoverable => "task_owner_gone_verb_not_recoverable",
            Self::CallerNotTaskOwner => "caller_is_not_task_owner_or_plan_owner",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::OwnerEpochStale => {
                "the presented owner epoch is not this task's current owner epoch"
            }
            Self::CallerBindingStale => "task mutation caller binding is stale or rebound",
            Self::OwnerGoneVerbNotRecoverable => {
                "the task's owning tab is gone and this verb is not recovery-eligible"
            }
            Self::CallerNotTaskOwner => {
                "the caller is neither this task's live owner session nor the live Plan owner"
            }
        }
    }
}

fn authority_refusal(kind: TaskAuthorityRefusal) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: kind.message().into(),
        data: Some(serde_json::json!({ "type": kind.discriminant() })),
    }
}

fn plan_caller(params: &Option<Value>) -> crate::service::AuthenticatedPlanCaller {
    let field = |key: &str| {
        params
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    crate::service::AuthenticatedPlanCaller {
        tab_id: field("caller_tab_id"),
        session_id: field("session_id"),
        binding_nonce: field("session_binding_nonce"),
    }
}

pub(super) fn plan_orchestrator_close_override(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
    plan_id: &str,
) -> bool {
    method == "plan.task.close" && caller_is_live_plan_owner(svc, plan_id, params)
}

/// The recovered status a `plan.task.update` asks for, so cancel and pend land
/// in the same write as the retirement. Reassign and respawn carry no status.
fn recovery_outcome(method: &str, params: &Option<Value>) -> Option<crate::plan::TaskStatus> {
    if method != "plan.task.update" {
        return None;
    }
    match params
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)?
    {
        "pending" => Some(crate::plan::TaskStatus::Pending),
        "cancelled" => Some(crate::plan::TaskStatus::Cancelled),
        _ => None,
    }
}

/// The owner counts as LIVE only when its recorded tab still exists AND that
/// tab's CURRENT agent session is the exact one the task bound. A lingering tab
/// with no session, or one rebound to a new session, is a GONE owner — the same
/// verdict reconcile/lane_liveness reach via `!live_agent`/session mismatch.
fn owner_session_is_live(
    svc: &ServiceState,
    owner_tab_id: Option<&str>,
    owner_session_id: Option<&str>,
) -> bool {
    let (Some(tab_id), Some(session_id)) = (owner_tab_id, owner_session_id) else {
        return false;
    };
    svc.mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| tab.agent_session_id.as_deref() == Some(session_id))
}

fn metadata_update_preserves_startup_owner(
    svc: &ServiceState,
    plan_id: &str,
    task: &crate::plan::Task,
) -> bool {
    // Dispatch records task ownership before the launch session is persisted.
    if task.interruption.owner_session_id.is_some() {
        return false;
    }
    let Some(tab_id) = task.interruption.owner_tab_id.as_deref() else {
        return false;
    };
    let Some(owner) = svc
        .plan_store
        .dispatch_startup_owner_candidate(plan_id, &task.id)
        .filter(|owner| owner.tab_id == tab_id)
    else {
        return false;
    };
    svc.plan_store
        .worker_startup_attempt_owns(plan_id, &task.id, tab_id, &owner.attempt_id, None)
        && (svc.plan_store.task_dispatch_startup_reservation_owns(
            plan_id,
            &task.id,
            &owner.attempt_id,
            tab_id,
        ) || super::plan_task_dispatch::dispatch_startup_owner_is_live(
            svc, plan_id, &task.id, tab_id,
        ))
}

fn lane_metadata_only_task_update(method: &str, params: &Option<Value>) -> bool {
    if method != "plan.task.update" {
        return false;
    }
    let Some(root) = params.as_ref().and_then(Value::as_object) else {
        return false;
    };
    let Some(lane) = root.get("lane").and_then(Value::as_object) else {
        return false;
    };
    if lane.is_empty()
        || lane.contains_key("tab_id")
        || !lane.keys().all(|key| {
            matches!(
                key.as_str(),
                "destination" | "cleanup_status" | "cleanup_hints"
            )
        })
    {
        return false;
    }
    root.keys().all(|key| {
        matches!(
            key.as_str(),
            "plan_id"
                | "task_id"
                | "caller_tab_id"
                | "session_id"
                | "session_binding_nonce"
                | "owner_epoch"
                | "lane"
        )
    })
}

/// True only for a live Plan owner reclaiming a task whose owner SESSION is gone
/// (tab absent, or tab present with a dead/rebound session). Keyed on owner
/// identity, not bare tab presence, so it agrees with reconcile's TabGone.
pub(super) fn plan_owner_may_recover(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
    plan_id: &str,
    owner_tab_id: Option<&str>,
    owner_session_id: Option<&str>,
) -> bool {
    if !is_recovery_method(method) {
        return false;
    }
    if owner_session_is_live(svc, owner_tab_id, owner_session_id) {
        return false;
    }
    caller_is_live_plan_owner(svc, plan_id, params)
}

pub(super) fn verify_owner_epoch_for_task_mutation(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<(), JsonRpcError> {
    if matches!(
        method,
        "task.list"
            | "task.show"
            | "task.add"
            | "plan.task.attempts"
            | "plan.task.attempt.show"
            | "plan.task.adopt_checkpoint"
            | "plan.task.add"
    ) {
        return Ok(());
    }
    let (_, task_id) = plan_refs::resolve_task(&svc.plan_store, params)?;
    let detail = svc
        .plan_store
        .show_task(&task_id)
        .ok_or_else(|| rpc_error(PlanError::TaskNotFound.to_string()))?;
    if plan_orchestrator_close_override(method, params, svc, &detail.plan_id) {
        return Ok(());
    }
    if detail.task.interruption.owner_epoch == 0 {
        return Ok(());
    }
    // Structure-only, so the live attempt is neither fenced nor retired: the
    // Planner edits the brief around a Worker that keeps its ownership.
    if planner_may_edit_structure(method, params, svc, &detail.plan_id) {
        return Ok(());
    }
    let metadata_update_preserves_startup = lane_metadata_only_task_update(method, params)
        && metadata_update_preserves_startup_owner(svc, &detail.plan_id, &detail.task);
    if plan_owner_may_recover(
        method,
        params,
        svc,
        &detail.plan_id,
        detail.task.interruption.owner_tab_id.as_deref(),
        detail.task.interruption.owner_session_id.as_deref(),
    ) && !metadata_update_preserves_startup
    {
        // Retire-and-recover is ONE transition: the verb cannot proceed until the
        // stale attempt is fenced, so a later dispatch_ready is already dead.
        svc.plan_store
            .retire_stale_task_owner(
                &detail.plan_id,
                &task_id,
                recovery_outcome(method, params),
                |_| false,
                epoch_ms(),
            )
            .map_err(|error| rpc_error(error.to_string()))?;
        return Ok(());
    }
    let authority = task_mutation_authority(params, svc)?;
    if detail.task.interruption.owner_epoch != authority.owner_epoch {
        return Err(authority_refusal(TaskAuthorityRefusal::OwnerEpochStale));
    }
    let binding_matches = svc
        .mux
        .read()
        .get_tab(&authority.tab_id)
        .is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(&authority.session_id)
                && tab.session_binding_nonce.as_deref() == Some(&authority.session_binding_nonce)
                && tab.reported_session_binding_nonce.as_deref()
                    == Some(&authority.session_binding_nonce)
        });
    if !binding_matches {
        return Err(authority_refusal(TaskAuthorityRefusal::CallerBindingStale));
    }
    // Plan ownership is proven against the LIVE tab, never against the owner
    // record's session field, which only the agent-restart path ever refreshes.
    if is_plan_owner_authority_method(method)
        && caller_is_live_plan_owner(svc, &detail.plan_id, params)
    {
        return Ok(());
    }
    svc.verify_task_owner_session(
        &task_id,
        &authority.tab_id,
        &authority.session_id,
        authority.owner_epoch,
    )
    .map_err(|_| {
        authority_refusal(unrecognized_authority(
            svc,
            &detail.plan_id,
            params,
            detail.task.interruption.owner_tab_id.as_deref(),
            detail.task.interruption.owner_session_id.as_deref(),
        ))
    })
}

fn caller_is_live_plan_owner(svc: &ServiceState, plan_id: &str, params: &Option<Value>) -> bool {
    svc.authorize_plan_orchestrator(plan_id, &plan_caller(params))
        .is_ok()
}

/// Names the state that actually blocked the caller: a Plan owner held off a
/// tab-gone task by verb eligibility, or a caller with no claim on the task.
fn unrecognized_authority(
    svc: &ServiceState,
    plan_id: &str,
    params: &Option<Value>,
    owner_tab_id: Option<&str>,
    owner_session_id: Option<&str>,
) -> TaskAuthorityRefusal {
    let owner_gone = !owner_session_is_live(svc, owner_tab_id, owner_session_id);
    if owner_gone && caller_is_live_plan_owner(svc, plan_id, params) {
        return TaskAuthorityRefusal::OwnerGoneVerbNotRecoverable;
    }
    TaskAuthorityRefusal::CallerNotTaskOwner
}

pub(super) fn task_mutation_authority(
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<TaskMutationAuthority, JsonRpcError> {
    let required = |field: &str| {
        params
            .as_ref()
            .and_then(|value| value.get(field))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: format!("task mutation requires {field}"),
                data: None,
            })
    };
    let tab_id = required("caller_tab_id")?;
    let session_binding_nonce = required("session_binding_nonce")?;
    let requested_session = params
        .as_ref()
        .and_then(|value| value.get("session_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let session_id = svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| {
            let live_session = tab.agent_session_id.as_deref()?;
            (requested_session.is_none_or(|requested| requested == live_session)
                && tab.session_binding_nonce.as_deref() == Some(session_binding_nonce.as_str())
                && tab.reported_session_binding_nonce.as_deref()
                    == Some(session_binding_nonce.as_str()))
            .then(|| live_session.to_string())
        })
        .ok_or_else(|| authority_refusal(TaskAuthorityRefusal::CallerBindingStale))?;
    Ok(TaskMutationAuthority {
        tab_id,
        session_id,
        session_binding_nonce,
        owner_epoch: params
            .as_ref()
            .and_then(|value| value.get("owner_epoch"))
            .and_then(Value::as_u64)
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "task mutation requires owner_epoch".into(),
                data: None,
            })?,
    })
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn rpc_error(message: impl ToString) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: message.to_string(),
        data: None,
    }
}

#[cfg(test)]
mod planner_authority_tests;
#[cfg(test)]
mod tests;
