use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

#[cfg(test)]
use codeterm_service_core::plan::Plan;
use codeterm_service_core::plan::{AgentRef, PlanEvidenceClass, ReviewCapacityClass};
use codeterm_service_core::service::plan::PlanError;
use codeterm_service_core::service::{ServiceContext, ServiceState};

mod qa_authority;
mod todo_reopen;

use super::JsonRpcError;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: e.rpc_code(),
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

fn emit_plan_changed(svc: &ServiceState, plan_id: &str) {
    svc.emit_committed_plan_changed(&plan_id);
}

fn continuation_binding_nonce(
    svc: &ServiceContext,
    executor_tab_id: &str,
    session_id: &str,
) -> Option<String> {
    let mux = svc.mux.read();
    mux.get_tab(executor_tab_id).and_then(|tab| {
        (tab.agent_session_id.as_deref() == Some(session_id)
            && tab.session_binding_nonce.is_some()
            && tab.session_binding_nonce == tab.reported_session_binding_nonce)
            .then(|| tab.session_binding_nonce.clone())
            .flatten()
    })
}

fn executor_tab_is_absent_local(
    svc: &ServiceContext,
    executor_tab_id: &str,
    mesh_peer: Option<&str>,
) -> bool {
    if mesh_peer.is_some_and(|peer| !peer.trim().is_empty()) {
        return false;
    }
    !svc.mux
        .read()
        .all_tabs()
        .any(|tab| tab.id == executor_tab_id)
}

fn actor(params: Option<&Value>) -> AgentRef {
    params
        .and_then(|x| x.get("from"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .or_else(|| {
            params
                .and_then(|x| x.get("caller_tab_id"))
                .and_then(|v| v.as_str())
                .map(AgentRef::from_tab)
        })
        .unwrap_or_else(|| AgentRef::from_tab("unknown"))
}

fn unauthorized(message: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32003,
        message: message.into(),
        data: Some(json!({ "type": "plan_orchestrator_authorization_required" })),
    }
}

fn structural_caller(
    params: Option<&Value>,
) -> codeterm_service_core::service::AuthenticatedPlanCaller {
    let field = |key: &str| {
        let value = str_field(params, key);
        (!value.is_empty()).then(|| value.to_string())
    };
    codeterm_service_core::service::AuthenticatedPlanCaller {
        tab_id: field("caller_tab_id"),
        session_id: field("session_id"),
        binding_nonce: field("session_binding_nonce"),
    }
}

/// Authorization is the canonical owner predicate, never a private copy of it.
/// The actor is then derived from the tab that predicate already validated.
fn authorized_orchestrator_actor(
    svc: &ServiceContext,
    plan_id: &str,
    params: Option<&Value>,
) -> Result<AgentRef, JsonRpcError> {
    // Runs before any mux guard of ours: it takes its own, and parking_lot reads
    // are not recursive.
    svc.authorize_plan_orchestrator(plan_id, &structural_caller(params))
        .map_err(|error| {
            super::plan_task_authority::execution_authority_error(
                error,
                Some("plan_orchestrator_authorization_required"),
            )
        })?;
    let mux = svc.mux.read();
    let tab = mux
        .get_tab(str_field(params, "caller_tab_id"))
        .ok_or_else(|| unauthorized("the claimed Orchestrator tab is not live"))?;
    Ok(AgentRef {
        tab_id: Some(tab.id.clone()),
        session_id: tab.agent_session_id.clone(),
        title: Some(tab.title.clone()),
    })
}

fn note(params: Option<&Value>) -> &str {
    params
        .and_then(|x| x.get("note").or_else(|| x.get("text")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn bool_field(params: Option<&Value>, key: &str) -> bool {
    params
        .and_then(|x| x.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn str_field<'a>(params: Option<&'a Value>, key: &str) -> &'a str {
    params
        .and_then(|x| x.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn finish_continuation_result(
    finished: bool,
    delivery: Result<(), String>,
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    review_round: u32,
) -> Result<(), JsonRpcError> {
    if !finished {
        let delivery_error = delivery.as_ref().err().cloned();
        let mut data = json!({
            "type": "bounce_continuation_finish_refused",
            "plan_id": plan_id,
            "task_id": task_id,
            "attempt_id": attempt_id,
            "review_round": review_round,
            "delivery_succeeded": delivery.is_ok(),
            "uncertain_completion": true,
        });
        if let Some(error) = delivery_error {
            data["delivery_error"] = json!(error);
        }
        return Err(JsonRpcError {
            code: -32603,
            message: format!(
                "ORCHESTRATION PRODUCT INCIDENT: review bounce continuation finish refused for {plan_id}/{task_id}/{attempt_id}; delivery outcome does not prove completion"
            ),
            data: Some(data),
        });
    }
    if let Err(error) = delivery {
        return Err(bounce_continuation_failure(
            plan_id,
            task_id,
            attempt_id,
            review_round,
            error,
        ));
    }
    Ok(())
}

fn bounce_continuation_failure(
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    review_round: u32,
    error: String,
) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: format!(
            "ORCHESTRATION PRODUCT INCIDENT: review bounce continuation failed for {plan_id}/{task_id}/{attempt_id}: {error}"
        ),
        data: Some(json!({
            "type": "bounce_continuation_failed",
            "plan_id": plan_id,
            "task_id": task_id,
            "attempt_id": attempt_id,
            "review_round": review_round,
            "error": error,
        })),
    }
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    let now = epoch_ms();
    match method {
        "plan.review.status" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let status = store.review_gate_status(&plan_id).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "review_gate": status }))
        }
        "plan.review.waves" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let waves = store.qa_waves(&plan_id).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "waves": waves }))
        }
        "plan.review.override" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            store
                .record_review_override(
                    &plan_id,
                    &task_id,
                    str_field(p, "owner"),
                    str_field(p, "reason"),
                    str_field(p, "reproduction"),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(svc, &plan_id);
            Ok(json!({ "plan_id": plan_id, "task_id": task_id, "recorded": true }))
        }
        "plan.review.reconcile-credit" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let from = authorized_orchestrator_actor(svc, &plan_id, p)?;
            let result = store
                .reconcile_review_credit(&plan_id, str_field(p, "reason"), from, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(svc, &plan_id);
            serde_json::to_value(result).map_err(|error| JsonRpcError {
                code: -32603,
                message: error.to_string(),
                data: None,
            })
        }
        "plan.review.authorize-capacity" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let from = authorized_orchestrator_actor(svc, &plan_id, p)?;
            let class = serde_json::from_value::<ReviewCapacityClass>(json!(str_field(p, "class")))
                .map_err(|_| JsonRpcError {
                    code: -32602,
                    message: "class must be review or canonical_gate".into(),
                    data: None,
                })?;
            let result = store
                .grant_review_capacity(&plan_id, &task_id, class, from, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(svc, &plan_id);
            serde_json::to_value(result).map_err(|error| JsonRpcError {
                code: -32603,
                message: error.to_string(),
                data: None,
            })
        }
        "plan.review.authorize-reporter" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let class = serde_json::from_value::<PlanEvidenceClass>(json!(str_field(p, "class")))
                .map_err(|_| JsonRpcError {
                code: -32602,
                message: "class must be review or canonical_qa".into(),
                data: None,
            })?;
            let result = svc
                .grant_plan_evidence_reporter(
                    &plan_id,
                    str_field(p, "reporter_tab_id"),
                    class,
                    &structural_caller(p),
                    now,
                )
                .map_err(|message| unauthorized(&message))?;
            emit_plan_changed(svc, &plan_id);
            serde_json::to_value(result).map_err(|error| JsonRpcError {
                code: -32603,
                message: error.to_string(),
                data: None,
            })
        }
        "plan.review.delegate" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let qa_tab_id = str_field(p, "qa_tab_id").trim();
            if qa_tab_id.is_empty() {
                return Err(JsonRpcError {
                    code: -32602,
                    message: "qa_tab_id is required".into(),
                    data: None,
                });
            }
            let result = qa_authority::delegate_review_task(svc, &plan_id, &task_id, qa_tab_id, p)?;
            emit_plan_changed(svc, &plan_id);
            Ok(result)
        }
        "plan.review.accept" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let residual_risk = p
                .and_then(|x| x.get("residual_risk"))
                .and_then(|v| v.as_str());
            let from = qa_authority::authorized_review_actor(svc, &plan_id, &task_id, p)?;
            if note(p).trim().is_empty() {
                return Err(plan_rpc_err(PlanError::QaNoteRequired));
            }
            let integration = store
                .task_lane_integration_projection(&plan_id, &task_id)
                .map_err(plan_rpc_err)?;
            super::plan_task_dispatch::canonical_writer::refuse_lane_integration_while_canonical_held(integration.as_ref())?;
            let worker = store
                .task_review_worker_projection(&plan_id, &task_id)
                .map_err(plan_rpc_err)?;
            let result = store
                .accept_review(
                    &plan_id,
                    &task_id,
                    note(p),
                    bool_field(p, "changed_executable_surfaces"),
                    bool_field(p, "distinct_oracle"),
                    residual_risk,
                    from.clone(),
                    now,
                )
                .map_err(plan_rpc_err)?;
            qa_authority::consume_qa_review_grant(svc, &from, &plan_id, &task_id);
            if let Some(worker) = worker {
                codeterm_service_core::async_runtime::block_on(svc.tab_todo_apply_review(
                    &worker,
                    from.tab_id.as_deref(),
                    true,
                ))
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
                if let Some(ports) =
                    svc.try_ext::<codeterm_service_core::ports::sentinel::SentinelPorts>()
                {
                    if let Err(error) = ports.cleanup.remove_worker_watchers(&worker) {
                        log::error!("[blueprint] accepted task watcher cleanup failed: {error}");
                    }
                }
            }
            emit_plan_changed(svc, &plan_id);
            svc.settle_lane(&plan_id, &task_id);
            svc.record_kpi_review_verdict(
                from.tab_id.as_deref().unwrap_or("unknown"),
                &plan_id,
                &task_id,
                codeterm_service_core::agent::kpi_ledger::KpiEventKind::Accept,
                result.review_round,
                note(p).len(),
            );
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.review.bounce" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let from = qa_authority::authorized_review_actor(svc, &plan_id, &task_id, p)?;
            if note(p).trim().is_empty() {
                return Err(plan_rpc_err(PlanError::QaNoteRequired));
            }
            if let Some(continuation) = store
                .closed_review_continuation(&plan_id, &task_id)
                .map_err(plan_rpc_err)?
            {
                if !executor_tab_is_absent_local(
                    svc,
                    &continuation.executor_tab_id,
                    continuation.mesh_peer.as_deref(),
                ) {
                    return Err(bounce_continuation_failure(
                        &plan_id,
                        &task_id,
                        &continuation.attempt_id,
                        continuation.review_round,
                        "executor may still own the task lane".into(),
                    ));
                }
            }
            codeterm_service_core::service::restore_review_attempt_binding(
                svc, &plan_id, &task_id, now,
            )
            .map_err(plan_rpc_err)?;
            todo_reopen::reopen_named_item(svc, p, &plan_id, &task_id, &from, note(p))?;
            let (mut result, recorded) = store
                .bounce_review_with_recorded_status(
                    &plan_id,
                    &task_id,
                    note(p),
                    bool_field(p, "changed_executable_surfaces"),
                    bool_field(p, "distinct_oracle"),
                    from.clone(),
                    now,
                )
                .map_err(plan_rpc_err)?;
            if recorded {
                if let Some(worker) = store
                    .task_review_worker_projection(&plan_id, &task_id)
                    .map_err(plan_rpc_err)?
                {
                    codeterm_service_core::async_runtime::block_on(svc.tab_todo_apply_review(
                        &worker,
                        from.tab_id.as_deref(),
                        false,
                    ))
                    .map_err(|message| JsonRpcError {
                        code: -32603,
                        message,
                        data: None,
                    })?;
                }
            }
            qa_authority::consume_qa_review_grant(svc, &from, &plan_id, &task_id);
            if recorded {
                svc.record_kpi_review_verdict(
                    from.tab_id.as_deref().unwrap_or("unknown"),
                    &plan_id,
                    &task_id,
                    codeterm_service_core::agent::kpi_ledger::KpiEventKind::Bounce,
                    result.review_round,
                    note(p).len(),
                );
            }
            let Some(mut continuation) = result.continuation.clone() else {
                emit_plan_changed(svc, &plan_id);
                return serde_json::to_value(result).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                });
            };
            let binding_nonce = continuation_binding_nonce(
                svc,
                &continuation.executor_tab_id,
                &continuation.session_id,
            );
            if binding_nonce.is_none()
                && executor_tab_is_absent_local(
                    svc,
                    &continuation.executor_tab_id,
                    continuation.mesh_peer.as_deref(),
                )
            {
                store
                    .reopen_review_continuation(
                        &plan_id,
                        &task_id,
                        &continuation.attempt_id,
                        continuation.epoch,
                        epoch_ms(),
                    )
                    .map_err(plan_rpc_err)?;
                result.continuation = None;
                emit_plan_changed(svc, &plan_id);
                return serde_json::to_value(result).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                });
            }
            store
                .claim_review_continuation_delivery(
                    &plan_id,
                    &task_id,
                    &continuation.attempt_id,
                    continuation.epoch,
                    now,
                )
                .map_err(plan_rpc_err)?;
            let delivery = match binding_nonce.as_deref() {
                Some(binding_nonce) => {
                    codeterm_service_core::service::messenger::Messenger::deliver_planner_session(
                        svc,
                        &continuation.executor_tab_id,
                        &continuation.session_id,
                        binding_nonce,
                        codeterm_service_core::service::messenger::MessageEnvelope::codeterm_reminder(
                            "review-bounce",
                        )
                        .id(Some(format!(
                            "{}:{}:{}",
                            continuation.attempt_id, continuation.review_round, continuation.epoch
                        ))),
                        note(p),
                        format!(
                            "review-bounce:{}:{}:{}:{}",
                            plan_id, task_id, continuation.attempt_id, continuation.review_round
                        ),
                    )
                    .map(|_| ())
                }
                None => Err("authenticated Worker continuation binding is unavailable".into()),
            };
            let delivered = delivery.is_ok();
            let finished = store
                .finish_review_continuation(
                    &plan_id,
                    &task_id,
                    &continuation.attempt_id,
                    continuation.epoch,
                    delivered,
                    epoch_ms(),
                )
                .map_err(plan_rpc_err)?;
            if !delivered
                && executor_tab_is_absent_local(
                    svc,
                    &continuation.executor_tab_id,
                    continuation.mesh_peer.as_deref(),
                )
            {
                store
                    .reopen_review_continuation(
                        &plan_id,
                        &task_id,
                        &continuation.attempt_id,
                        continuation.epoch,
                        epoch_ms(),
                    )
                    .map_err(plan_rpc_err)?;
                result.continuation = None;
                emit_plan_changed(svc, &plan_id);
                return serde_json::to_value(result).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                });
            }
            emit_plan_changed(svc, &plan_id);
            finish_continuation_result(
                finished,
                delivery,
                &plan_id,
                &task_id,
                &continuation.attempt_id,
                continuation.review_round,
            )?;
            continuation.state = "continued".into();
            result.continuation = Some(continuation);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.review.hold" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let from = actor(p);
            if from
                .tab_id
                .as_deref()
                .is_some_and(|tab_id| qa_authority::is_qa(svc, tab_id))
            {
                return Err(unauthorized("QAs may only accept or bounce handed reviews"));
            }
            let result = store
                .hold_review(&plan_id, &task_id, note(p), from, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(svc, &plan_id);
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: format!("Unknown plan review method: {method}"),
            data: None,
        }),
    }
}

#[cfg(test)]
mod authority_tests;

#[cfg(test)]
mod cleaned_runtime_tests;

#[cfg(test)]
mod tests;
