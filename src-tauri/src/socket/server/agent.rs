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
            let source_outcome = params
                .as_ref()
                .and_then(|p| p.get("report_outcome"))
                .and_then(|value| {
                    serde_json::from_value::<codeterm_types::agent::ReportOutcomeClass>(
                        value.clone(),
                    )
                    .ok()
                });
            // the freeform message; absent keys deserialize to None.
            let details = params
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
                .unwrap_or_default();
            let svc_ref = &**svc;
            let result = svc_ref
                .submit_report_with_source_outcome(
                    &tab_id,
                    &message,
                    details,
                    plan_id.as_deref(),
                    task_id.as_deref(),
                    milestone_kind.as_deref(),
                    source_outcome,
                )
                .map_err(|e| JsonRpcError {
                    code: e.code,
                    message: e.message,
                    data: e.data,
                })?;
            let mut resp = serde_json::json!({
                "ok": true,
                "accepted": true,
                "state": "delivered",
                "report_id": result.report_id,
                "target_tab_id": result.orchestrator_tab_id,
                "report_outcome": result.report_outcome,
            });
            if let Some(diagnostics) = result.diagnostics {
                resp["diagnostics"] = serde_json::to_value(diagnostics).unwrap_or_default();
            }
            if let Some((plan_id, task_id)) = result.persisted_task {
                resp["task_report"] = serde_json::json!({
                    "persisted": true,
                    "plan_id": plan_id,
                    "task_id": task_id,
                    "report_outcome": result.report_outcome,
                });
            }
            if let Some((plan_id, stride_id)) = result.persisted_plan_evidence {
                resp["plan_report"] = serde_json::json!({
                    "persisted": true,
                    "plan_id": plan_id,
                    "stride_id": stride_id,
                    "task_id": serde_json::Value::Null,
                    "task_transition": false,
                });
            }
            if let Some(rejection) = result.rejected_task_transition {
                resp["task_report"] = serde_json::json!({
                    "persisted": false,
                    "plan_id": rejection.plan_id,
                    "task_id": rejection.task_id,
                    "transition_rejected": true,
                    "cause": rejection.cause,
                    "report_outcome": rejection.report_outcome,
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
            let acknowledged = match svc.plan_store.acknowledge_live_task_attempt_for_tab(
                &tab_id,
                plan_id,
                task_id,
                epoch_ms(),
            ) {
                Ok(acknowledged) => {
                    log_ack_event(
                        "task_ack",
                        "accepted",
                        "none",
                        Some(&acknowledged.attempt.executor_tab_id),
                        Some(&acknowledged.attempt.id),
                    );
                    acknowledged
                }
                Err(error) => {
                    let message = error.to_string();
                    log_ack_event(
                        "task_ack",
                        "refused",
                        task_ack_refusal_class(&message),
                        None,
                        None,
                    );
                    return Err(JsonRpcError {
                        code: -32603,
                        message,
                        data: None,
                    });
                }
            };
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

        "agent.startup_ack" => {
            let caller_tab_id = get_string_param(params, "caller_tab_id")?;
            let attempt_id = get_string_param(params, "attempt_id")?;
            let binding_nonce = get_string_param(params, "binding_nonce")?;
            let accepted =
                match svc.acknowledge_agent_startup(&caller_tab_id, &attempt_id, &binding_nonce) {
                    Ok(accepted) => accepted,
                    Err(message) => {
                        log_ack_event(
                            "startup_ack",
                            "refused",
                            "startup_ack_not_owner",
                            None,
                            None,
                        );
                        return Err(JsonRpcError {
                            code: -32603,
                            message,
                            data: None,
                        });
                    }
                };
            if !accepted {
                log_ack_event(
                    "startup_ack",
                    "refused",
                    "startup_ack_not_awaiting",
                    None,
                    None,
                );
                return Err(JsonRpcError {
                    code: -32603,
                    message: format!(
                        "startup attempt {attempt_id} for Tab {caller_tab_id} is not awaiting acknowledgement"
                    ),
                    data: None,
                });
            }
            log_ack_event(
                "startup_ack",
                "accepted",
                "none",
                Some(&caller_tab_id),
                Some(&attempt_id),
            );
            Ok(serde_json::json!({
                "ok": true,
                "accepted": true,
                "state": "startup_acknowledged",
                "tab_id": caller_tab_id,
                "attempt_id": attempt_id,
            }))
        }

        "agent.startup_hold" => {
            let caller_tab_id = get_string_param(params, "caller_tab_id")?;
            let tab_id = get_string_param(params, "tab_id")?;
            let attempt_id = get_string_param(params, "attempt_id")?;
            svc.hold_agent_startup(&caller_tab_id, &tab_id, &attempt_id)
                .map_err(|rejection| JsonRpcError {
                    code: -32603,
                    message: rejection.message,
                    data: Some(rejection.provenance),
                })
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

fn log_ack_event(
    stage: &str,
    outcome: &str,
    refusal_class: &str,
    tab_id: Option<&str>,
    attempt_id: Option<&str>,
) {
    let record = ack_log_record(stage, outcome, refusal_class, tab_id, attempt_id);
    log::info!(target: "codeterm::worker_ack", "{record}");
}

fn ack_log_record(
    stage: &str,
    outcome: &str,
    refusal_class: &str,
    tab_id: Option<&str>,
    attempt_id: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "event": "worker_ack",
        "stage": stage,
        "outcome": outcome,
        "refusal_class": refusal_class,
        "timestamp_ms": epoch_ms(),
        "tab_id": tab_id,
        "attempt_id": attempt_id,
    })
}

fn task_ack_refusal_class(message: &str) -> &'static str {
    if message.contains("worker_ack_attempt_superseded") {
        "worker_ack_attempt_superseded"
    } else if message.contains("worker_ack_ambiguous_live_attempts") {
        "worker_ack_ambiguous_live_attempts"
    } else if message.contains("worker_ack_no_live_attempt") {
        "worker_ack_no_live_attempt"
    } else if message.contains("worker_ack_plan_not_active") {
        "worker_ack_plan_not_active"
    } else if message.contains("worker_ack_task_not_open") {
        "worker_ack_task_not_open"
    } else if message.contains("attempt was not acknowledged") {
        "worker_ack_not_acknowledged"
    } else if message.contains("attempt missing") {
        "worker_ack_attempt_missing"
    } else {
        "worker_ack_refused"
    }
}

#[cfg(test)]
mod tests {
    use super::{ack_log_record, task_ack_refusal_class};

    #[test]
    fn agent_ack_diagnostic_startup_accepted() {
        let record = ack_log_record(
            "startup_ack",
            "accepted",
            "none",
            Some("tab-safe"),
            Some("attempt-safe"),
        );
        assert_eq!(record["stage"], "startup_ack");
        assert_eq!(record["outcome"], "accepted");
        assert_eq!(record["attempt_id"], "attempt-safe");
    }

    #[test]
    fn agent_ack_diagnostic_task_accepted() {
        let record = ack_log_record(
            "task_ack",
            "accepted",
            "none",
            Some("tab-safe"),
            Some("attempt-safe"),
        );
        assert_eq!(record["stage"], "task_ack");
        assert_eq!(record["outcome"], "accepted");
        assert_eq!(record["tab_id"], "tab-safe");
    }

    #[test]
    fn agent_ack_diagnostic_refused_classified() {
        assert_eq!(
            task_ack_refusal_class(
                "worker_ack_attempt_superseded: token=secret binding_nonce=private"
            ),
            "worker_ack_attempt_superseded"
        );
        assert_eq!(
            task_ack_refusal_class("worker_ack_ambiguous_live_attempts: plan/task details"),
            "worker_ack_ambiguous_live_attempts"
        );
        assert_eq!(
            task_ack_refusal_class("unexpected provider key material"),
            "worker_ack_refused"
        );
    }

    #[test]
    fn agent_ack_diagnostic_stale_unattributed() {
        let record = ack_log_record("task_ack", "refused", "worker_ack_refused", None, None);
        assert_eq!(record["tab_id"], serde_json::Value::Null);
        assert_eq!(record["attempt_id"], serde_json::Value::Null);
    }

    #[test]
    fn agent_ack_diagnostic_unauthenticated_unattributed() {
        let record = ack_log_record(
            "startup_ack",
            "refused",
            "startup_ack_not_owner",
            None,
            None,
        );
        assert_eq!(record["tab_id"], serde_json::Value::Null);
        assert_eq!(record["attempt_id"], serde_json::Value::Null);
        assert_eq!(record["refusal_class"], "startup_ack_not_owner");
    }

    #[test]
    fn agent_ack_diagnostic_redacts_secrets() {
        let raw_error =
            "worker_ack_attempt_superseded: token=secret binding_nonce=private raw_error=hidden";
        let record = ack_log_record(
            "task_ack",
            "refused",
            task_ack_refusal_class(raw_error),
            None,
            None,
        );
        assert!(!record.to_string().contains("binding_nonce"));
        assert!(!record.to_string().contains("token"));
        assert!(!record.to_string().contains("raw_error"));
        assert!(!record.to_string().contains("secret"));
        assert!(!record.to_string().contains("private"));
    }
}
