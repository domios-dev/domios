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
    svc.authorize_draft_structure_mutation(
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
    .map_err(|message| JsonRpcError {
        code: -32003,
        message,
        data: Some(serde_json::json!({ "type": "plan_draft_authorization_required" })),
    })
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
    let authority = task_mutation_authority(params)?;
    if detail.task.interruption.owner_epoch != authority.owner_epoch {
        return Err(rpc_error("task mutation owner epoch is stale"));
    }
    let binding_matches = svc.mux.read().get_tab(&authority.tab_id).is_some_and(|tab| {
        tab.agent_session_id.as_deref() == Some(&authority.session_id)
            && tab.session_binding_nonce.as_deref() == Some(&authority.session_binding_nonce)
            && tab.reported_session_binding_nonce.as_deref()
                == Some(&authority.session_binding_nonce)
    });
    if !binding_matches {
        return Err(rpc_error("task mutation caller binding is stale or rebound"));
    }
    let orchestrator = svc.plan_store.owner_of(&detail.plan_id).is_some_and(|owner| {
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
    Ok(TaskMutationAuthority {
        tab_id: required("caller_tab_id")?,
        session_id: required("session_id")?,
        session_binding_nonce: required("session_binding_nonce")?,
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

fn rpc_error(message: impl ToString) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: message.to_string(),
        data: None,
    }
}
