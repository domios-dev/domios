//! `agent.*` JSON-RPC handlers.

use serde_json::Value;

use codeterm_service_core::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "agent.acp" => codeterm_service_core::service::agent_runtime::acp_profiles::execute(
            svc,
            params.as_ref().unwrap_or(&Value::Null),
        )
        .map_err(|message| JsonRpcError {
            code: -32602,
            message,
            data: None,
        }),
        "agent.reparent" => super::remote_team_rpc::reparent(params, svc),
        "agent.turns" => {
            let target = get_string_param(params, "tab_id")?;
            let params = params.as_ref().ok_or(JsonRpcError {
                code: -32602,
                message: "params required".into(),
                data: None,
            })?;
            let last = params
                .get("last")
                .map(|v| v.as_u64().ok_or("last must be an integer"))
                .transpose()
                .map_err(|message| JsonRpcError {
                    code: -32602,
                    message: message.into(),
                    data: None,
                })?
                .unwrap_or(10);
            let after = params
                .get("after_offset")
                .map(|v| {
                    v.as_u64()
                        .ok_or("after_offset must be a nonnegative integer")
                })
                .transpose()
                .map_err(|message| JsonRpcError {
                    code: -32602,
                    message: message.into(),
                    data: None,
                })?
                .unwrap_or(0);
            let caller = params.get("caller_tab_id").and_then(Value::as_str);
            let nonce = params.get("session_binding_nonce").and_then(Value::as_str);
            let result = svc
                .read_tool_less_session_turns(&target, last, after, caller, nonce)
                .map_err(|message| JsonRpcError {
                    code: -32003,
                    message,
                    data: None,
                })?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "agent.diagnostic" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let kind = get_string_param(params, "kind")?;
            if kind != "report_refused" {
                return Err(JsonRpcError {
                    code: -32602,
                    message: "unsupported agent diagnostic kind".into(),
                    data: None,
                });
            }
            let message = get_string_param(params, "message")?;
            let target_tab_id = svc
                .send_refused_report_diagnostic(&tab_id, &message)
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            Ok(serde_json::json!({
                "ok": true,
                "delivered": true,
                "kind": kind,
                "target_tab_id": target_tab_id,
            }))
        }
        "agent.spawn" => {
            let p = params.as_ref().ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let mut spawn_params = svc.spawn_params_from_json(p).map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
                data: None,
            })?;
            codeterm_service_core::async_runtime::block_on(
                svc.inherit_neighbor_agent_spawn(&mut spawn_params),
            )
            .map_err(|message| JsonRpcError {
                code: -32003,
                message,
                data: None,
            })?;
            codeterm_service_core::service::spawn::validate_agent_model(
                &svc.agent_registry,
                spawn_params.agent.as_deref().unwrap_or_default(),
                spawn_params.model.as_deref(),
            )
            .map_err(super::model_errors::invalid_model_rpc_error)?;
            let default_provider = svc.configuration().general_agent.agent_provider;
            svc.validate_spawn_options(
                spawn_params.agent.as_deref().unwrap_or(&default_provider),
                &spawn_params,
            )
            .map_err(super::model_errors::spawn_target_rpc_error)?;
            let plan_task = spawn_params.plan_task.clone();

            let svc_ref = &**svc;
            let result = svc_ref
                .spawn_agent_sync(spawn_params)
                .map_err(super::model_errors::spawn_rpc_error)?;
            svc.emit_mux_state_changed_empty();
            let ack_warning = if let Some(task_ref) = plan_task.as_deref() {
                match svc.arm_bound_worker_ack_deadline_for_tab(
                    &result.tab_id,
                    format!("domios agent spawn --plan-task {task_ref}"),
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
            let launch = svc
                .await_spawn_launch_verdict(
                    &result.tab_id,
                    codeterm_service_core::service::spawn_launch_verdict::SPAWN_LAUNCH_VERDICT_DEADLINE,
                )
                .map_err(|failure| JsonRpcError {
                    code: -32603,
                    message: failure.message(),
                    data: Some(failure.data()),
                })?;

            let mut resp = serde_json::json!({
                "tab_id": result.tab_id,
                "team_id": result.team_id,
                "agent": result.agent,
                "model": result.model,
                "title": result.title,
                "description": result.description,
                "role": result.role,
                "launch_state": launch.launch_state(),
            });
            if let Some(warning) = launch.merged_warning(&result.tab_id, result.warning.as_deref())
            {
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
            if let Some(hint) =
                codeterm_service_core::service::plan::hints::maybe_orchestration_reminder()
            {
                resp["_hints"] = serde_json::json!([hint]);
            }
            Ok(resp)
        }

        "agent.report" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let message = get_string_param(params, "message")?;
            if params
                .as_ref()
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str)
                == Some("report_refused")
            {
                let target_tab_id = svc
                    .send_refused_report_diagnostic(&tab_id, &message)
                    .map_err(|message| JsonRpcError {
                        code: -32603,
                        message,
                        data: None,
                    })?;
                return Ok(serde_json::json!({
                    "ok": true,
                    "accepted": false,
                    "state": "refused",
                    "kind": "report_refused",
                    "refused_report_diagnostic_sent": true,
                    "diagnostic_target_tab_id": target_tab_id,
                }));
            }
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
            let runtime_binding =
                ack_runtime_binding(params, &tab_id, svc).map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            let runtime_binding = runtime_binding
                .as_ref()
                .map(|(session, nonce)| (session.as_str(), nonce.as_str()));
            let mut acknowledged = match svc
                .plan_store
                .acknowledge_live_task_attempt_for_tab_with_runtime_binding(
                    &tab_id,
                    plan_id,
                    task_id,
                    runtime_binding,
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
            if acknowledged.already_acknowledged
                && (acknowledged.attempt.session_id.is_none()
                    || acknowledged.attempt.session_binding_nonce.is_none())
            {
                let completion = (|| {
                    let body = params.as_ref().ok_or("caller params are missing")?;
                    let nonce = body
                        .get("session_binding_nonce")
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .map(str::to_string)
                        .or_else(|| {
                            svc.mux
                                .read()
                                .get_tab(&tab_id)
                                .and_then(|tab| tab.session_binding_nonce.clone())
                        })
                        .ok_or("acknowledging tab has no current launch nonce")?;
                    let session = body
                        .get("caller_session_id")
                        .map(|value| {
                            value
                                .as_str()
                                .filter(|value| !value.trim().is_empty())
                                .ok_or("caller session claim is invalid")
                        })
                        .transpose()?;
                    if body.get("attempt_id").is_some_and(|value| {
                        value.as_str() != Some(acknowledged.attempt.id.as_str())
                    }) {
                        return Err(
                            "explicit attempt claim conflicts with current assignment".into()
                        );
                    }
                    svc.complete_acknowledged_dispatch_binding(
                        &acknowledged,
                        &nonce,
                        session,
                        epoch_ms(),
                    )
                })();
                acknowledged.attempt = completion.map_err(|cause| {
                    super::plan_skills_rpc::attempt_binding_refusal(&acknowledged, &cause)
                })?;
            }
            let plan_id = acknowledged.plan_id.clone();
            let task_id = acknowledged.task_id.clone();
            svc.acknowledge_worker_ack(&plan_id, &task_id, &tab_id, &acknowledged.attempt);
            if acknowledged.already_acknowledged {
                let response = task_ack_success_response(
                    &plan_id,
                    &task_id,
                    &tab_id,
                    &acknowledged.attempt.id,
                    acknowledged.attempt.acknowledged_at,
                    acknowledged.attempt.session_id.is_some(),
                    true,
                );
                return task_ack_with_skill_brief(response, params, svc, &acknowledged);
            }
            svc.gateway.require_worker_report(&tab_id);
            super::plan::emit_plan_changed(svc, &plan_id);
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
            let response = task_ack_success_response(
                &plan_id,
                &task_id,
                &tab_id,
                &acknowledged.attempt.id,
                acknowledged.attempt.acknowledged_at,
                runtime_ready,
                false,
            );
            task_ack_with_skill_brief(response, params, svc, &acknowledged)
        }

        "agent.startup_ack" => {
            let caller_tab_id = get_string_param(params, "caller_tab_id")?;
            let attempt_id = params
                .as_ref()
                .and_then(|params| params.get("attempt_id"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let binding_nonce = get_string_param(params, "binding_nonce")?;
            let acknowledged = match attempt_id.as_deref() {
                Some(attempt_id) => {
                    svc.acknowledge_agent_startup(&caller_tab_id, attempt_id, &binding_nonce)
                }
                None => svc.acknowledge_attemptless_agent_startup(&caller_tab_id, &binding_nonce),
            };
            let attempt_id = attempt_id.unwrap_or_default();
            let acknowledgment = match acknowledged {
                Ok(acknowledgment) => acknowledgment,
                Err(message) => {
                    let refusal_class = if message.contains("not awaiting acknowledgement") {
                        "startup_ack_not_awaiting"
                    } else {
                        "startup_ack_not_owner"
                    };
                    log_ack_event("startup_ack", "refused", refusal_class, None, None);
                    return Err(JsonRpcError {
                        code: -32603,
                        message,
                        data: None,
                    });
                }
            };
            let not_required = matches!(
                acknowledgment,
                codeterm_service_core::service::worker_ack::AgentStartupAcknowledgement::NotRequired
            );
            log_ack_event(
                "startup_ack",
                if not_required {
                    "not_required"
                } else {
                    "accepted"
                },
                "none",
                Some(&caller_tab_id),
                Some(&attempt_id),
            );
            Ok(startup_ack_success_response(
                &caller_tab_id,
                &attempt_id,
                acknowledgment,
            ))
        }

        "agent.inbound_claim" => {
            let caller_tab_id = get_string_param(params, "caller_tab_id")?;
            let binding_nonce = get_string_param(params, "session_binding_nonce")?;
            let claim_id = get_string_param(params, "claim_id")?;
            let text = svc
                .claim_inbound_tool_messages(&caller_tab_id, &binding_nonce, &claim_id)
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            Ok(serde_json::json!({ "text": text }))
        }

        "agent.inbound_settle" => {
            let caller_tab_id = get_string_param(params, "caller_tab_id")?;
            let binding_nonce = get_string_param(params, "session_binding_nonce")?;
            let claim_id = get_string_param(params, "claim_id")?;
            let written = params
                .as_ref()
                .and_then(|params| params.get("written"))
                .and_then(Value::as_bool)
                .ok_or_else(|| JsonRpcError {
                    code: -32602,
                    message: "written must be a boolean".into(),
                    data: None,
                })?;
            svc.settle_inbound_tool_messages(&caller_tab_id, &binding_nonce, &claim_id, written)
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            Ok(serde_json::json!({ "settled": true }))
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

fn ack_runtime_binding(
    params: &Option<Value>,
    tab_id: &str,
    svc: &ServiceState,
) -> Result<Option<(String, String)>, String> {
    let claimed_session = optional_ack_identity_claim(params, "caller_session_id")?;
    let claimed_nonce = optional_ack_identity_claim(params, "session_binding_nonce")?;
    let (session_id, configured_nonce, reported_nonce, booting) = {
        let mux = svc.mux.read();
        let tab = mux
            .get_tab(tab_id)
            .ok_or_else(|| "acknowledging tab is not live".to_string())?;
        (
            tab.agent_session_id.clone(),
            tab.session_binding_nonce.clone(),
            tab.reported_session_binding_nonce.clone(),
            tab.agent_booting,
        )
    };
    let live = session_id
        .as_deref()
        .filter(|session| !session.trim().is_empty())
        .zip(
            configured_nonce
                .as_deref()
                .filter(|nonce| !nonce.trim().is_empty())
                .filter(|nonce| reported_nonce.as_deref() == Some(*nonce)),
        );
    let Some((session_id, nonce)) = live else {
        if booting
            && session_id.is_none()
            && reported_nonce.is_none()
            && claimed_session.is_none()
            && claimed_nonce.is_some_and(|nonce| configured_nonce.as_deref() == Some(nonce))
        {
            return Ok(None);
        }
        return if claimed_session.is_some() || claimed_nonce.is_some() {
            Err("acknowledging tab has no verified current launch binding".into())
        } else {
            Ok(None)
        };
    };
    if claimed_session.is_some_and(|claim| claim != session_id)
        || claimed_nonce.is_some_and(|claim| claim != nonce)
    {
        return Err("acknowledging tab identity claim is stale or foreign".into());
    }
    Ok(Some((session_id.into(), nonce.into())))
}

fn optional_ack_identity_claim<'a>(
    params: &'a Option<Value>,
    field: &str,
) -> Result<Option<&'a str>, String> {
    params
        .as_ref()
        .and_then(|body| body.get(field))
        .map(|value| {
            value
                .as_str()
                .filter(|claim| !claim.trim().is_empty())
                .ok_or_else(|| format!("acknowledging {field} claim is invalid"))
        })
        .transpose()
}

fn task_ack_success_response(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    acknowledged_at: Option<u64>,
    runtime_ready: bool,
    already_acknowledged: bool,
) -> Value {
    let state = if already_acknowledged {
        "already_acknowledged"
    } else if runtime_ready {
        "in_progress"
    } else {
        "awaiting_runtime_ready"
    };
    serde_json::json!({
        "ok": true,
        "accepted": true,
        "already_acknowledged": already_acknowledged,
        "state": state,
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "acknowledged_at": acknowledged_at,
    })
}

fn task_ack_with_skill_brief(
    mut response: Value,
    params: &Option<Value>,
    svc: &ServiceState,
    acknowledged: &codeterm_service_core::service::plan::task_attempts::TaskAttemptAcknowledgement,
) -> Result<Value, JsonRpcError> {
    let mux = svc.mux.read();
    let mut identity = params.clone().unwrap_or_else(|| serde_json::json!({}));
    let attempt_bound = acknowledged.attempt.session_id.is_some()
        && acknowledged.attempt.session_binding_nonce.is_some();
    if attempt_bound && !identity.get("session_binding_nonce").is_some() {
        let tab = mux
            .get_tab(&acknowledged.attempt.executor_tab_id)
            .ok_or_else(|| {
                super::plan_skills_rpc::attempt_binding_refusal(
                    acknowledged,
                    "acknowledging tab is missing",
                )
            })?;
        let nonce = tab
            .session_binding_nonce
            .as_deref()
            .filter(|nonce| tab.reported_session_binding_nonce.as_deref() == Some(*nonce))
            .filter(|nonce| acknowledged.attempt.session_binding_nonce.as_deref() == Some(*nonce))
            .ok_or_else(|| {
                super::plan_skills_rpc::attempt_binding_refusal(
                    acknowledged,
                    "current launch nonce is not reported by the acknowledging tab",
                )
            })?;
        let session = tab
            .agent_session_id
            .as_deref()
            .filter(|session| acknowledged.attempt.session_id.as_deref() == Some(*session))
            .ok_or_else(|| {
                super::plan_skills_rpc::attempt_binding_refusal(
                    acknowledged,
                    "current provider session differs from the attempt",
                )
            })?;
        if !identity.get("tab_id").is_some() {
            identity["tab_id"] = serde_json::json!(acknowledged.attempt.executor_tab_id);
        }
        if !identity.get("caller_session_id").is_some() {
            identity["caller_session_id"] = serde_json::json!(session);
        }
        identity["session_binding_nonce"] = serde_json::json!(nonce);
    }
    let brief = super::plan_skills_rpc::ack_skill_brief(
        &svc.plan_store,
        &mux,
        &Some(identity),
        acknowledged,
        epoch_ms(),
    )?;
    if let (Some(response), Some(brief)) = (response.as_object_mut(), brief.as_object()) {
        response.extend(brief.clone());
    }
    Ok(response)
}

fn startup_ack_success_response(
    tab_id: &str,
    attempt_id: &str,
    acknowledgment: codeterm_service_core::service::worker_ack::AgentStartupAcknowledgement,
) -> Value {
    if acknowledgment
        == codeterm_service_core::service::worker_ack::AgentStartupAcknowledgement::NotRequired
    {
        return serde_json::json!({
            "ok": true,
            "accepted": false,
            "state": "not_required",
        });
    }
    let already_acknowledged = acknowledgment
        == codeterm_service_core::service::worker_ack::AgentStartupAcknowledgement::AlreadyAcknowledged;
    let state = if already_acknowledged {
        "already_acknowledged"
    } else {
        "startup_acknowledged"
    };
    serde_json::json!({
        "ok": true,
        "accepted": true,
        "already_acknowledged": already_acknowledged,
        "state": state,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
    })
}

#[cfg(test)]
#[path = "agent/tests.rs"]
mod tests;
