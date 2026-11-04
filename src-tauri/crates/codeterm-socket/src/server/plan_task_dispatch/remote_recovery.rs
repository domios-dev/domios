use serde_json::{json, Value};

use crate::server::JsonRpcError;
use codeterm_service_core::plan::TaskExecutionAttempt;
use codeterm_service_core::service::plan::{RemoteRecoveryAttemptRead, RemoteWorkerBindingRead};
use codeterm_service_core::service::ServiceState;

pub(crate) fn observe_worker(
    svc: &ServiceState,
    plan: &str,
    task: &str,
    attempt: &RemoteWorkerBindingRead,
    presented_nonce: Option<&str>,
    close: bool,
) -> Result<Value, String> {
    let peer = attempt
        .mesh_peer
        .as_deref()
        .ok_or("attempt has no mesh peer")?;
    let client = svc
        .ext::<codeterm_service_core::MeshStateHandle>()
        .lock()
        .client_for(peer)
        .ok_or("remote peer has no signed route")?;
    codeterm_service_core::async_runtime::block_on(client.auth_post_json_within(
        "/api/mesh/worker-observation",
        &json!({
            "plan_id": plan, "task_id": task, "attempt_id": attempt.id,
            "worker_tab_id": attempt.executor_tab_id,
            "recorded_nonce": attempt.session_binding_nonce,
            "presented_nonce": presented_nonce, "close": close,
        }),
        None,
    ))
}

fn remote_recovery_decision(
    attempt: &RemoteRecoveryAttemptRead,
    now: u64,
    replace_unacked: bool,
) -> (bool, bool) {
    let awaiting = attempt.acknowledged_at.is_none()
        && attempt.remote_dispatch_state
            == Some(codeterm_service_core::plan::RemoteDispatchState::AwaitingWorkerAck);
    let expired = awaiting
        && attempt.assigned_at.is_some_and(|assigned| {
            now.saturating_sub(assigned)
                >= codeterm_service_core::service::worker_ack::REMOTE_WORKER_ACK_DEADLINE
                    .as_millis() as u64
        });
    (expired, awaiting && replace_unacked)
}

pub(crate) fn recover_remote_attempts(
    svc: &ServiceState,
    plan: &str,
    task: &str,
    now: u64,
    replace_unacked: bool,
) -> Result<(), JsonRpcError> {
    let attempts = svc
        .plan_store
        .remote_recovery_attempts_projection(plan, task)
        .map_err(super::super::plan::plan_rpc_err)?;
    for attempt in &attempts {
        let (expired, replacing) = remote_recovery_decision(attempt, now, replace_unacked);
        let observed = observe_worker(
            svc,
            plan,
            task,
            &attempt.binding,
            None,
            expired || replacing,
        );
        if replacing && !expired {
            if let Err(error) = &observed {
                return Err(super::invalid_dispatch(format!(
                    "remote Worker replacement could not confirm closure: {error}"
                )));
            }
        }
        let missing = observed
            .as_ref()
            .is_ok_and(|value| value.get("exists").and_then(Value::as_bool) == Some(false));
        if !missing && !expired {
            continue;
        }
        if let Err(error) = observed {
            if !expired {
                continue;
            }
            log::debug!(
                "[remote-recovery] expired attempt {} peer observation failed: {error}",
                attempt.binding.id
            );
        }
        svc.plan_store
            .retire_remote_task_attempt(
                plan,
                task,
                &attempt.binding.id,
                attempt.binding.mesh_peer.as_deref().unwrap_or_default(),
                &attempt.binding.executor_tab_id,
                if replacing {
                    "remote_unacked_replaced"
                } else if expired {
                    "remote_ack_timeout"
                } else {
                    "remote_tab_closed"
                },
                now,
            )
            .map_err(super::super::plan::plan_rpc_err)?;
    }
    Ok(())
}

pub(crate) fn respawn_remote(
    params: &Option<Value>,
    svc: &ServiceState,
    plan: &str,
    task: &str,
    attempt: &TaskExecutionAttempt,
    now: u64,
) -> Result<Value, JsonRpcError> {
    if params
        .as_ref()
        .and_then(|body| body.get("launch_profile"))
        .and_then(Value::as_str)
        .is_some()
        || attempt.launch_profile.is_some()
    {
        return Err(super::invalid_dispatch(
            "launch_profile selection is local-only for task respawn; the recorded profile cannot be dropped",
        ));
    }
    recover_remote_attempts(svc, plan, task, now, true)?;
    let source = svc
        .plan_store
        .show_task_attempt(plan, task, &attempt.id)
        .map_err(super::super::plan::plan_rpc_err)?;
    if source.attempt.closed_at.is_none() {
        return Err(super::invalid_dispatch(
            "remote attempt still has a live Worker; close its remote tab before respawning",
        ));
    }
    let forwarded = remote_respawn_params(params, attempt);
    super::remote::dispatch(&Some(forwarded), svc, now)
}

fn remote_respawn_params(params: &Option<Value>, attempt: &TaskExecutionAttempt) -> Value {
    let mut forwarded = params.clone().unwrap_or_else(|| json!({}));
    forwarded["mesh_peer"] = json!(attempt.mesh_peer);
    forwarded["execution_intent"] = json!("mutating");
    forwarded["authorized_gates"] = json!(attempt.authorized_gates.as_deref().unwrap_or(&[]));
    if let Some(provider) = forwarded
        .get("provider")
        .and_then(Value::as_str)
        .map(|provider| json!(provider))
        .or_else(|| attempt.provider.as_ref().map(|provider| json!(provider)))
    {
        forwarded["agent"] = provider;
    }
    if forwarded.get("model").and_then(Value::as_str).is_none() {
        if let Some(model) = &attempt.model {
            forwarded["model"] = json!(model);
        }
    }
    if forwarded
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .is_none()
    {
        forwarded["reasoning_effort"] = json!(attempt.reasoning_effort);
    }
    forwarded
}

#[cfg(test)]
mod tests;
