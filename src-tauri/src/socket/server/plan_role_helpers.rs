use serde_json::Value;

use crate::service::plan::task_attempts::{TaskAttemptCreated, TaskRespawnDraft};
use crate::service::plan::{PlanError, PlanStore};
use crate::service::ServiceState;

use super::plan_fields::optional_string_field;
use super::{get_string_param, JsonRpcError};

pub(super) fn planner_role_for_pane(svc: &ServiceState, pane_id: &str) -> Option<&'static str> {
    svc.mux.read().get_pane(pane_id).and_then(|pane| {
        (pane.role == Some(crate::multiplexer::types::AgentRole::Manager)
            && pane.role_profile.as_deref() == Some("planner"))
        .then_some("planner")
    })
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
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
    compensate_failed_attempt(record(), || svc.close_pane(tab_id))
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
    now: u64,
) -> Result<String, JsonRpcError> {
    let (id, tid) = super::plan_refs::resolve_task(store, params)?;
    let pane = get_string_param(params, "pane")?;
    store
        .assign_task_with_role(
            &id,
            &tid,
            &pane,
            body.and_then(|x| x.get("peer")).and_then(|v| v.as_str()),
            planner_role_for_pane(svc, &pane),
            now,
        )
        .map_err(plan_rpc_err)?;
    Ok(id)
}

pub(super) trait RespawnAttemptRoleExt {
    fn record_task_respawn_attempt_with_profile(
        &self,
        plan_id: &str,
        task_id: &str,
        pane_id: &str,
        draft: &TaskRespawnDraft,
        svc: &ServiceState,
        now: u64,
    ) -> Result<TaskAttemptCreated, PlanError>;
}

impl RespawnAttemptRoleExt for PlanStore {
    fn record_task_respawn_attempt_with_profile(
        &self,
        plan_id: &str,
        task_id: &str,
        pane_id: &str,
        draft: &TaskRespawnDraft,
        svc: &ServiceState,
        now: u64,
    ) -> Result<TaskAttemptCreated, PlanError> {
        self.record_task_respawn_attempt(
            plan_id,
            task_id,
            pane_id,
            draft,
            planner_role_for_pane(svc, pane_id).or(Some("worker")),
            now,
        )
    }
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
