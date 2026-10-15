use codeterm_service_core::app_runtime::AppHandle;
use serde_json::{json, Value};

use codeterm_service_core::service::ServiceState;
use codeterm_service_core::service::{
    restore_attempt_binding_for_caller, AttemptBindingRestoreResult, AttemptBindingRestoreTarget,
};

pub(super) fn is_targeted(params: &Option<Value>) -> bool {
    ["task_id", "attempt_id", "owner_epoch"].iter().any(|key| {
        params
            .as_ref()
            .is_some_and(|value| value.get(*key).is_some())
    })
}

pub(super) fn dispatch(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, super::JsonRpcError> {
    let plan_id = required_target(params, "plan_id")?;
    let task_id = required_target(params, "task_id")?;
    let attempt_id = required_target(params, "attempt_id")?;
    let owner_epoch = params
        .as_ref()
        .and_then(|value| value.get("owner_epoch"))
        .and_then(Value::as_u64)
        .filter(|epoch| *epoch > 0)
        .ok_or_else(incomplete)?;
    if params
        .as_ref()
        .and_then(Value::as_object)
        .is_some_and(|object| {
            object.keys().any(|key| {
                key.starts_with("target_")
                    || matches!(
                        key.as_str(),
                        "tab_id"
                            | "expected_old_nonce"
                            | "current_nonce"
                            | "nonce_override"
                            | "session_id_override"
                            | "lineage"
                            | "lineage_proof"
                            | "lineage_entries"
                            | "host_lineage"
                            | "prior_session_binding_nonces"
                    )
            })
        })
    {
        return Err(incomplete());
    }
    if params
        .as_ref()
        .and_then(|value| value.get("authenticated_mesh_peer_id"))
        .is_some()
    {
        return Err(refused(
            "attempt_binding_restore_local_only",
            "same-attempt binding restoration is local-only",
        ));
    }
    if params
        .as_ref()
        .and_then(|value| value.get("session_id"))
        .is_some_and(|value| {
            value
                .as_str()
                .is_none_or(|session| session.trim().is_empty())
        })
    {
        return Err(super::super::plan::plan_rpc_err(
            codeterm_service_core::service::plan::AttemptBindingRefusalKind::CallerSessionMismatch
                .refusal(),
        ));
    }
    let caller = super::caller(params);
    let outcome = restore_attempt_binding_for_caller(
        svc,
        AttemptBindingRestoreTarget {
            plan_id: &plan_id,
            task_id: &task_id,
            attempt_id: &attempt_id,
            owner_epoch,
        },
        &caller,
        super::epoch_ms(),
    )
    .map_err(super::super::plan::plan_rpc_err)?;
    let status = match outcome {
        AttemptBindingRestoreResult::Restored(_) => {
            super::emit_plan_changed(app, &plan_id);
            "restored"
        }
        AttemptBindingRestoreResult::AlreadyCurrent => "already_current",
    };
    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "status": status,
    }))
}

fn required_target(params: &Option<Value>, key: &str) -> Result<String, super::JsonRpcError> {
    params
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(incomplete)
}

fn incomplete() -> super::JsonRpcError {
    refused(
        "attempt_binding_target_incomplete",
        "targeted rebind requires exact plan, task, attempt, and current owner epoch",
    )
}

fn refused(kind: &str, message: &str) -> super::JsonRpcError {
    super::JsonRpcError {
        code: -32003,
        message: message.into(),
        data: Some(json!({ "type": kind })),
    }
}

#[cfg(test)]
mod tests;
