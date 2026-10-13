use serde_json::{json, Value};
use tauri::Manager;

use crate::plan::TaskExecutionAttempt;
use crate::service::ServiceState;
use crate::socket::server::JsonRpcError;

pub(crate) fn observe_worker(
    svc: &ServiceState,
    plan: &str,
    task: &str,
    attempt: &TaskExecutionAttempt,
    presented_nonce: Option<&str>,
    close: bool,
) -> Result<Value, String> {
    let peer = attempt
        .mesh_peer
        .as_deref()
        .ok_or("attempt has no mesh peer")?;
    let client = svc
        .app_handle
        .state::<crate::MeshStateHandle>()
        .lock()
        .client_for(peer)
        .ok_or("remote peer has no signed route")?;
    tauri::async_runtime::block_on(client.auth_post_json_within(
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
    attempt: &TaskExecutionAttempt,
    now: u64,
    replace_unacked: bool,
) -> (bool, bool) {
    let awaiting = attempt.acknowledged_at.is_none()
        && attempt.remote_dispatch_state
            == Some(crate::plan::RemoteDispatchState::AwaitingWorkerAck);
    let expired = awaiting
        && attempt.assigned_at.is_some_and(|assigned| {
            now.saturating_sub(assigned)
                >= crate::service::worker_ack::REMOTE_WORKER_ACK_DEADLINE.as_millis() as u64
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
    let detail = svc.plan_store.show_task(task).ok_or_else(|| {
        super::super::plan::plan_rpc_err(crate::service::plan::PlanError::TaskNotFound)
    })?;
    for attempt in detail.task.execution_attempts.iter().filter(|attempt| {
        attempt.mesh_peer.is_some()
            && attempt.closed_at.is_none()
            && attempt.retired_at.is_none()
            && attempt.report.is_none()
    }) {
        let (expired, replacing) = remote_recovery_decision(attempt, now, replace_unacked);
        let observed = observe_worker(svc, plan, task, attempt, None, expired || replacing);
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
                attempt.id
            );
        }
        svc.plan_store
            .retire_remote_task_attempt(
                plan,
                task,
                &attempt.id,
                attempt.mesh_peer.as_deref().unwrap_or_default(),
                &attempt.executor_tab_id,
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
    app: &tauri::AppHandle,
    svc: &ServiceState,
    plan: &str,
    task: &str,
    attempt: &TaskExecutionAttempt,
    now: u64,
) -> Result<Value, JsonRpcError> {
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
    super::remote::dispatch(&Some(forwarded), app, svc, now)
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
mod tests {
    use super::*;

    #[test]
    fn remote_recovery_requires_ack_deadline_or_explicit_replacement() {
        let mut attempt = TaskExecutionAttempt {
            assigned_at: Some(1),
            remote_dispatch_state: Some(crate::plan::RemoteDispatchState::AwaitingWorkerAck),
            ..Default::default()
        };
        let deadline =
            crate::service::worker_ack::REMOTE_WORKER_ACK_DEADLINE.as_millis() as u64 + 1;
        assert_eq!(
            remote_recovery_decision(&attempt, deadline - 1, false),
            (false, false)
        );
        assert_eq!(
            remote_recovery_decision(&attempt, deadline, false),
            (true, false)
        );
        assert_eq!(remote_recovery_decision(&attempt, 2, true), (false, true));
        attempt.acknowledged_at = Some(2);
        assert_eq!(
            remote_recovery_decision(&attempt, deadline, true),
            (false, false)
        );
    }

    #[test]
    fn remote_respawn_keeps_peer_gates_and_respects_model_override() {
        let attempt = TaskExecutionAttempt {
            mesh_peer: Some("paired-peer-id".into()),
            provider: Some("provider".into()),
            model: Some("original-model".into()),
            reasoning_effort: Some("high".into()),
            authorized_gates: Some(vec!["gate".into()]),
            ..Default::default()
        };
        let params = remote_respawn_params(
            &Some(json!({"plan_id": "plan", "task_id": "task", "model": "replacement-model"})),
            &attempt,
        );
        assert_eq!(params["mesh_peer"], "paired-peer-id");
        assert_eq!(params["execution_intent"], "mutating");
        assert_eq!(params["agent"], "provider");
        assert_eq!(params["model"], "replacement-model");
        assert_eq!(params["reasoning_effort"], "high");
        assert_eq!(params["authorized_gates"], json!(["gate"]));
        assert_eq!(params["plan_id"], "plan");
        let rest_params =
            remote_respawn_params(&Some(json!({"provider": null, "model": null})), &attempt);
        assert_eq!(rest_params["agent"], "provider");
        assert_eq!(rest_params["model"], "original-model");
    }
}
