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
    "plan.task.respawn",
    "plan.task.close",
];

pub(super) fn is_recovery_method(method: &str) -> bool {
    RECOVERY_METHODS.contains(&method)
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
    svc.authorize_plan_orchestrator(plan_id, &plan_caller(params))
        .is_ok()
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
    if detail.task.interruption.owner_epoch == 0 {
        return Ok(());
    }
    if plan_owner_may_recover(
        method,
        params,
        svc,
        &detail.plan_id,
        detail.task.interruption.owner_tab_id.as_deref(),
        detail.task.interruption.owner_session_id.as_deref(),
    ) {
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
        return Err(rpc_error("task mutation owner epoch is stale"));
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
        return Err(rpc_error(
            "task mutation caller binding is stale or rebound",
        ));
    }
    let orchestrator = svc
        .plan_store
        .owner_of(&detail.plan_id)
        .is_some_and(|owner| {
            owner.tab_id.as_deref() == Some(&authority.tab_id)
                && owner.session_id.as_deref() == Some(&authority.session_id)
        });
    if orchestrator {
        return Ok(());
    }
    svc.verify_task_owner_session(
        &task_id,
        &authority.tab_id,
        &authority.session_id,
        authority.owner_epoch,
    )
    .map_err(rpc_error)
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
        .ok_or_else(|| rpc_error("task mutation caller binding is stale or rebound"))?;
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
