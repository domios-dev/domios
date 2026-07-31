//! `agent.*` JSON-RPC handlers.

use serde_json::Value;
use tauri::AppHandle;

use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    _app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "agent.spawn" => {
            let p = params.as_ref().ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let spawn_params =
                crate::service::SpawnAgentParams::from_json(p).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                    data: None,
                })?;
            let plan_task = spawn_params.plan_task.clone();

            let svc_ref = &**svc;
            let result = svc_ref
                .spawn_agent_sync(spawn_params)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            svc.emit_mux_state_changed_empty();
            let ack_warning = if let Some(task_ref) = plan_task.as_deref() {
                match svc.arm_bound_worker_ack_deadline_for_tab(
                    &result.tab_id,
                    format!("codeterm agent spawn --plan-task {task_ref}"),
                ) {
                    Ok(true) => None,
                    Ok(false) => Some(format!(
                        "worker ACK tracking was not armed for task-bound tab {}",
                        result.tab_id
                    )),
                    Err(error) => Some(format!(
                        "worker ACK tracking failed for tab {}: {error}",
                        result.tab_id
                    )),
                }
            } else {
                None
            };

            let mut resp = serde_json::json!({
                "tab_id": result.tab_id,
                "agent": result.agent,
                "model": result.model,
                "title": result.title,
                "description": result.description,
                "role": result.role,
            });
            if let Some(ref warning) = result.warning {
                resp["warning"] = serde_json::json!(warning);
            }
            if let Some(warning) = ack_warning {
                resp["orchestration_incident"] = serde_json::json!({
                    "kind": "worker_ack_tracking_failed",
                    "impact": warning,
                    "fallback": "Inspect the existing tab once. Do not blind-resend.",
                });
            }
            if let Some(ref notice) = result.budget_notice {
                resp["budget_notice"] = serde_json::to_value(notice).unwrap_or_default();
            }
            if let Some(hint) = crate::service::plan::hints::maybe_orchestration_reminder() {
                resp["_hints"] = serde_json::json!([hint]);
            }
            Ok(resp)
        }

        "agent.report" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let message = get_string_param(params, "message")?;
            let plan_id = params
                .as_ref()
                .and_then(|p| p.get("plan_id"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let task_id = params
                .as_ref()
                .and_then(|p| p.get("task_id"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let milestone_kind = params
                .as_ref()
                .and_then(|p| p.get("kind"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            // the freeform message; absent keys deserialize to None.
            let details = params
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
                .unwrap_or_default();
            let svc_ref = &**svc;
            let result = svc_ref
                .submit_report(
                    &tab_id,
                    &message,
                    details,
                    plan_id.as_deref(),
                    task_id.as_deref(),
                    milestone_kind.as_deref(),
                )
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            let mut resp = serde_json::json!({
                "ok": true,
                "accepted": true,
                "state": "recorded",
                "report_id": result.report_id,
                "target_tab_id": result.orchestrator_tab_id,
            });
            if let Some((plan_id, task_id)) = result.persisted_task {
                resp["task_report"] = serde_json::json!({
                    "persisted": true,
                    "plan_id": plan_id,
                    "task_id": task_id,
                });
            }
            if !result.notices.is_empty() {
                resp["notices"] = serde_json::json!(result.notices);
            }
            Ok(resp)
        }

        "agent.ack" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let plan_id = params
                .as_ref()
                .and_then(|value| value.get("plan_id"))
                .and_then(Value::as_str);
            let task_id = params
                .as_ref()
                .and_then(|value| value.get("task_id"))
                .and_then(Value::as_str);
            let acknowledged = svc
                .plan_store
                .acknowledge_live_task_attempt_for_tab(&tab_id, plan_id, task_id, epoch_ms())
                .map_err(|error| JsonRpcError {
                    code: -32603,
                    message: error.to_string(),
                    data: None,
                })?;
            let plan_id = acknowledged.plan_id.clone();
            let task_id = acknowledged.task_id.clone();
            svc.worker_ack
                .acknowledge(&tab_id, &acknowledged.attempt.id);
            svc.gateway.require_worker_report(&tab_id);
            super::plan::emit_plan_changed(_app, &plan_id);
            let runtime_ready = acknowledged.attempt.session_id.is_some();
            let parent_tab_id = acknowledged
                .attempt
                .orchestrator_tab_id
                .as_deref()
                .or(acknowledged.attempt.parent_tab_id.as_deref());
            if let Some(parent_tab_id) = parent_tab_id {
                let title = svc
                    .mux
                    .read()
                    .get_tab(&tab_id)
                    .map(|tab| tab.title.clone())
                    .unwrap_or_else(|| "Worker".into());
                let payload = serde_json::json!({
                    "kind": "worker_ack",
                    "plan_id": plan_id,
                    "task_id": task_id,
                    "tab_id": tab_id,
                    "attempt_id": acknowledged.attempt.id,
                    "acknowledged_at": acknowledged.attempt.acknowledged_at,
                    "state": if runtime_ready { "in_progress" } else { "awaiting_runtime_ready" },
                });
                super::plan_task_dispatch::publish_startup_report(
                    svc,
                    parent_tab_id,
                    &tab_id,
                    &title,
                    &plan_id,
                    &task_id,
                    payload,
                );
            }
            Ok(serde_json::json!({
                "ok": true,
                "accepted": true,
                "state": if runtime_ready { "in_progress" } else { "awaiting_runtime_ready" },
                "plan_id": plan_id,
                "task_id": task_id,
                "tab_id": tab_id,
                "attempt_id": acknowledged.attempt.id,
                "acknowledged_at": acknowledged.attempt.acknowledged_at,
            }))
        }

        "agent.ledger" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let svc_ref = &**svc;
            let rows = svc_ref.get_ledger(&tab_id);
            serde_json::to_value(&rows).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
