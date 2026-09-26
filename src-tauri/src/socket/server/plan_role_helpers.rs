use serde_json::Value;

use crate::service::plan::{PlanError, PlanStore};
use crate::service::ServiceState;

use super::plan_fields::optional_string_field;
use super::{get_string_param, JsonRpcError};

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: e.rpc_code(),
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

fn compensate_failed_attempt<T>(
    result: Result<T, PlanError>,
    close_spawned_tab: impl FnOnce() -> Result<(), String>,
) -> Result<T, JsonRpcError> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let cleanup_error = close_spawned_tab().err();
            let mut rpc_error = plan_rpc_err(error);
            if let Some(cleanup_error) = cleanup_error {
                rpc_error.message = format!(
                    "{}; spawned tab compensation failed: {cleanup_error}",
                    rpc_error.message
                );
            }
            Err(rpc_error)
        }
    }
}

pub(super) fn record_spawned_attempt<T>(
    svc: &ServiceState,
    tab_id: &str,
    record: impl FnOnce() -> Result<T, PlanError>,
) -> Result<T, JsonRpcError> {
    compensate_failed_attempt(record(), || svc.close_tab(tab_id))
}

/// A record failure whose compensating close is REPORTED, not just folded into
/// the message. A caller holding the tab's lane must know whether it is still live.
pub(super) struct SpawnedAttemptFailure {
    pub error: JsonRpcError,
    pub close_error: Option<String>,
}

/// Same compensation as `record_spawned_attempt`, but reports whether the tab
/// closed. A sibling on purpose: the shared helper's contract is unchanged.
pub(super) fn record_spawned_attempt_reporting_close<T>(
    svc: &ServiceState,
    tab_id: &str,
    record: impl FnOnce() -> Result<T, PlanError>,
) -> Result<T, SpawnedAttemptFailure> {
    match record() {
        Ok(value) => Ok(value),
        Err(error) => {
            let close_error = svc.close_tab(tab_id).err();
            let mut rpc_error = plan_rpc_err(error);
            if let Some(close_error) = close_error.as_deref() {
                rpc_error.message = format!(
                    "{}; spawned tab compensation failed: {close_error}",
                    rpc_error.message
                );
            }
            Err(SpawnedAttemptFailure {
                error: rpc_error,
                close_error,
            })
        }
    }
}

pub(super) fn update_role_guidance_if_present(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    params: Option<&Value>,
    now: u64,
) -> Result<(), JsonRpcError> {
    if params.and_then(|x| x.get("role_guidance")).is_some() {
        store
            .update_task_role_guidance(
                plan_id,
                task_id,
                optional_string_field(params, "role_guidance"),
                now,
            )
            .map_err(plan_rpc_err)?;
    }
    Ok(())
}

pub(super) fn update_personality_if_present(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    params: Option<&Value>,
    now: u64,
) -> Result<(), JsonRpcError> {
    if params.and_then(|x| x.get("personality")).is_some()
        || params
            .and_then(|x| x.get("personality_provenance"))
            .is_some()
    {
        let provenance = params
            .and_then(|x| x.get("personality_provenance"))
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid personality_provenance: {e}"),
                    data: None,
                })
            })
            .transpose()?;
        store
            .update_task_personality(
                plan_id,
                task_id,
                optional_string_field(params, "personality"),
                provenance,
                now,
            )
            .map_err(plan_rpc_err)?;
    }
    Ok(())
}

pub(super) fn assign_task_with_profile(
    store: &PlanStore,
    svc: &ServiceState,
    params: &Option<Value>,
    body: Option<&Value>,
    authority: Option<&crate::service::plan::interruption::TaskMutationAuthority>,
    now: u64,
) -> Result<String, JsonRpcError> {
    let (id, tid) = super::plan_refs::resolve_task(store, params)?;
    let tab = get_string_param(params, "tab")?;
    let peer = body.and_then(|x| x.get("peer")).and_then(|v| v.as_str());
    let assignment = {
        let mux = svc.mux.read();
        crate::service::task_assignment::prepare_existing_task_assignment(
            &mux, &tab, peer, authority,
        )
    }
    .map_err(plan_rpc_err)?;
    crate::service::task_assignment::persist_existing_task_assignment(
        store,
        assignment,
        &id,
        &tid,
        &tab,
        peer,
        authority,
        now,
    )
    .map_err(plan_rpc_err)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn failed_attempt_commit_closes_spawned_tab_exactly_once() {
        let open = AtomicBool::new(true);
        let closes = AtomicUsize::new(0);

        let result = compensate_failed_attempt::<()>(
            Err(PlanError::ReviewWipExceeded {
                wip_count: 1,
                wip_limit: 1,
                credit: -1,
            }),
            || {
                closes.fetch_add(1, Ordering::SeqCst);
                open.store(false, Ordering::SeqCst);
                Ok(())
            },
        );

        assert!(matches!(result, Err(JsonRpcError { code: -32603, .. })));
        assert!(!open.load(Ordering::SeqCst));
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }
}
