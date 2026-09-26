use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

#[cfg(test)]
use crate::plan::Plan;
use crate::plan::{AgentRef, PlanEvidenceClass, ReviewCapacityClass};
use crate::service::plan::PlanError;
use crate::service::{ServiceContext, ServiceState};

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

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
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

fn structural_caller(params: Option<&Value>) -> crate::service::AuthenticatedPlanCaller {
    let field = |key: &str| {
        let value = str_field(params, key);
        (!value.is_empty()).then(|| value.to_string())
    };
    crate::service::AuthenticatedPlanCaller {
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
        .map_err(|message| unauthorized(&message))?;
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
    app: &AppHandle,
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
            emit_plan_changed(app, &plan_id);
            Ok(json!({ "plan_id": plan_id, "task_id": task_id, "recorded": true }))
        }
        "plan.review.reconcile-credit" => {
            let plan_id = super::plan_refs::resolve_plan(store, params)?;
            let from = authorized_orchestrator_actor(svc, &plan_id, p)?;
            let result = store
                .reconcile_review_credit(&plan_id, str_field(p, "reason"), from, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
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
            emit_plan_changed(app, &plan_id);
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
            emit_plan_changed(app, &plan_id);
            serde_json::to_value(result).map_err(|error| JsonRpcError {
                code: -32603,
                message: error.to_string(),
                data: None,
            })
        }
        "plan.review.accept" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let residual_risk = p
                .and_then(|x| x.get("residual_risk"))
                .and_then(|v| v.as_str());
            let from = actor(p);
            if let Some(plan) = store.get(&plan_id) {
                super::plan_task_dispatch::canonical_writer::refuse_lane_integration_while_canonical_held(
                    &plan, &task_id,
                )?;
            }
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
            emit_plan_changed(app, &plan_id);
            svc.record_kpi_review_verdict(
                from.tab_id.as_deref().unwrap_or("unknown"),
                &plan_id,
                &task_id,
                crate::agent::kpi_ledger::KpiEventKind::Accept,
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
            if note(p).trim().is_empty() {
                return Err(plan_rpc_err(PlanError::QaNoteRequired));
            }
            if let Some(continuation) = store.closed_review_continuation(&plan_id, &task_id) {
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
            let from = actor(p);
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
                svc.record_kpi_review_verdict(
                    from.tab_id.as_deref().unwrap_or("unknown"),
                    &plan_id,
                    &task_id,
                    crate::agent::kpi_ledger::KpiEventKind::Bounce,
                    result.review_round,
                    note(p).len(),
                );
            }
            let Some(mut continuation) = result.continuation.clone() else {
                emit_plan_changed(app, &plan_id);
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
                emit_plan_changed(app, &plan_id);
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
                    crate::service::messenger::Messenger::deliver_planner_session(
                        svc,
                        &continuation.executor_tab_id,
                        &continuation.session_id,
                        binding_nonce,
                        crate::service::messenger::MessageEnvelope::codeterm_reminder(
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
                emit_plan_changed(app, &plan_id);
                return serde_json::to_value(result).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                });
            }
            emit_plan_changed(app, &plan_id);
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
            let result = store
                .hold_review(&plan_id, &task_id, note(p), from, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
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
mod tests {
    use super::*;
    use crate::plan::OrchestratorRef;
    use crate::service::plan::{NewTask, PlanStore};

    fn continuation_store() -> (tempfile::TempDir, PlanStore, String, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "continuation".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "worker".into(),
                    acceptance_criteria: vec!["fixed".into()],
                    test_plan: Some("oracle".into()),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        store
            .assign_task(&plan.id, &task_id, "worker", None, 3)
            .unwrap();
        let mut stored = store.get(&plan.id).unwrap();
        let task = stored
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.status = crate::plan::TaskStatus::InReview;
        let attempt = task.execution_attempts.last_mut().unwrap();
        let attempt_id = attempt.id.clone();
        attempt.session_id = Some("worker-session".into());
        attempt.acknowledged_at = Some(3);
        attempt.reported_at = Some(4);
        attempt.report = Some(Default::default());
        store.save(&mut stored, 4).unwrap();
        (dir, store, plan.id, task_id, attempt_id)
    }

    #[test]
    fn review_bounce_finish_false_is_an_uncertain_completion_refusal() {
        let error = finish_continuation_result(false, Ok(()), "plan-1", "task-1", "attempt-1", 2)
            .expect_err("a no-op finish must never serialize success");
        assert_eq!(error.code, -32603);
        let data = error.data.unwrap();
        assert_eq!(data["type"], "bounce_continuation_finish_refused");
        assert_eq!(data["delivery_succeeded"], true);
        assert_eq!(data["uncertain_completion"], true);
    }

    #[test]
    fn review_bounce_finish_success_and_delivery_failure_keep_distinct_results() {
        assert!(
            finish_continuation_result(true, Ok(()), "plan-1", "task-1", "attempt-1", 1,).is_ok()
        );

        let error = finish_continuation_result(
            true,
            Err("delivery unavailable".into()),
            "plan-1",
            "task-1",
            "attempt-1",
            1,
        )
        .expect_err("failed delivery must remain an explicit failure");
        let data = error.data.unwrap();
        assert_eq!(data["type"], "bounce_continuation_failed");
        assert_eq!(data["error"], "delivery unavailable");
        assert!(data.get("uncertain_completion").is_none());
    }

    #[test]
    fn review_bounce_reopens_exact_acknowledged_attempt_and_continues_once() {
        let (_dir, store, plan_id, task_id, attempt_id) = continuation_store();
        let before = store.show_task(&task_id).unwrap().task;
        let result = store
            .bounce_review(
                &plan_id,
                &task_id,
                "repair only this defect",
                true,
                true,
                AgentRef::from_tab("reviewer"),
                5,
            )
            .unwrap();
        let continuation = result.continuation.unwrap();
        assert_eq!(continuation.attempt_id, attempt_id);
        assert!(store
            .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 6)
            .is_ok());
        let refusal = store
            .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 7)
            .expect_err("second claim must be an explicit refusal");
        assert!(matches!(
            refusal,
            crate::service::plan::PlanError::ReviewContinuationDeliveryRefused {
                expected_epoch: 1,
                actual_epoch: Some(1),
                continuation_state: Some(ref state),
                ..
            } if state == "continuation_delivering"
        ));
        assert!(store
            .finish_review_continuation(&plan_id, &task_id, &attempt_id, 1, true, 8)
            .unwrap());
        let after = store.show_task(&task_id).unwrap().task;
        assert_eq!(after.status, crate::plan::TaskStatus::InProgress);
        assert_eq!(
            after.execution_attempts.len(),
            before.execution_attempts.len()
        );
        assert_eq!(after.execution_attempts[0].acknowledged_at, Some(3));
        assert_eq!(
            after.execution_attempts[0].continuation.delivered_at,
            Some(8)
        );
    }

    #[test]
    fn review_bounce_delivery_failure_stays_visible_and_never_fakes_in_progress() {
        let (_dir, store, plan_id, task_id, attempt_id) = continuation_store();
        store
            .bounce_review(
                &plan_id,
                &task_id,
                "repair",
                true,
                true,
                AgentRef::from_tab("reviewer"),
                5,
            )
            .unwrap();
        assert!(store
            .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 6)
            .is_ok());
        assert!(store
            .finish_review_continuation(&plan_id, &task_id, &attempt_id, 1, false, 7)
            .unwrap());
        let failed = store.show_task(&task_id).unwrap().task;
        assert_eq!(failed.status, crate::plan::TaskStatus::InReview);
        assert_eq!(
            failed.status_label.as_deref(),
            Some("bounce_continuation_failed")
        );
        assert_eq!(
            failed.execution_attempts[0].continuation.state.as_deref(),
            Some("bounce_continuation_failed")
        );
    }

    #[test]
    fn review_bounce_fences_stale_rebound_replacement_and_competition_attempts() {
        let (_dir, store, plan_id, task_id, _attempt_id) = continuation_store();
        let mut plan = store.get(&plan_id).unwrap();
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.assigned_tab_id = Some("replacement".into());
        store.save(&mut plan, 5).unwrap();
        let before = store.show_task(&task_id).unwrap().task;
        assert!(store
            .bounce_review(
                &plan_id,
                &task_id,
                "must not deliver",
                true,
                true,
                AgentRef::from_tab("reviewer"),
                6,
            )
            .is_err());
        assert_eq!(store.show_task(&task_id).unwrap().task, before);
    }

    #[test]
    fn review_bounce_replay_is_idempotent_for_same_review_round() {
        let (_dir, store, plan_id, task_id, attempt_id) = continuation_store();
        let (first, first_recorded) = store
            .bounce_review_with_recorded_status(
                &plan_id,
                &task_id,
                "repair",
                true,
                true,
                AgentRef::from_tab("reviewer"),
                5,
            )
            .unwrap();
        assert!(first_recorded);
        let (replay, replay_recorded) = store
            .bounce_review_with_recorded_status(
                &plan_id,
                &task_id,
                "repair",
                true,
                true,
                AgentRef::from_tab("reviewer"),
                6,
            )
            .unwrap();
        assert!(!replay_recorded);
        assert_eq!(first.milestone_id, replay.milestone_id);
        assert_eq!(first.continuation, replay.continuation);
        assert!(store
            .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 7)
            .is_ok());
        let refusal = store
            .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 8)
            .expect_err("replayed claim must be an explicit refusal");
        assert!(matches!(
            refusal,
            crate::service::plan::PlanError::ReviewContinuationDeliveryRefused { .. }
        ));
    }

    fn socket_bounce_fixture(
        worker_present: bool,
    ) -> (
        crate::service::test_fixture::ServiceFixture,
        String,
        String,
        String,
    ) {
        let (mut mux, default_tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let worker_tab_id = if worker_present {
            let tab = mux.get_tab_mut(&default_tab_id).unwrap();
            tab.agent_type = Some("codex".into());
            tab.agent_session_id = Some("worker-session-rotated".into());
            tab.pty_id = Some("worker-pty".into());
            tab.exit_suspected_at = Some(std::time::Instant::now());
            default_tab_id
        } else {
            "worker-tab-absent".into()
        };
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let store = &fixture.service.plan_store;
        let mut plan = Plan::new(
            "socket bounce".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "worker task".into(),
                    acceptance_criteria: vec!["fixed".into()],
                    test_plan: Some("verify the retry".into()),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        store
            .assign_task(&plan.id, &task_id, &worker_tab_id, Some("Worker"), 3)
            .unwrap();
        let lane_path = fixture.temp.path().join("worker-lane");
        let lane_path = lane_path.to_string_lossy().into_owned();
        let mut stored = store.get(&plan.id).unwrap();
        let task = stored
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.status = crate::plan::TaskStatus::InReview;
        task.lane = crate::plan::TaskLaneMetadata {
            tab_id: Some(worker_tab_id),
            worktree_path: Some(lane_path.clone()),
            ..Default::default()
        };
        let attempt = task.execution_attempts.last_mut().unwrap();
        attempt.session_id = Some("worker-session".into());
        attempt.acknowledged_at = Some(3);
        attempt.reported_at = Some(4);
        attempt.report = Some(Default::default());
        attempt.worktree_path = Some(lane_path.clone());
        store.save(&mut stored, 4).unwrap();
        (fixture, plan.id, task_id, lane_path)
    }

    fn dispatch_bounce(
        fixture: &crate::service::test_fixture::ServiceFixture,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Value, JsonRpcError> {
        let params = Some(json!({
            "plan_id": plan_id,
            "task_id": task_id,
            "note": "retry the same lane",
            "from": AgentRef::from_tab("reviewer"),
        }));
        let app = fixture._app.handle().clone();
        let svc = std::sync::Arc::new(fixture.service.clone());
        dispatch("plan.review.bounce", &params, &app, &svc)
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_review_reopens_when_executor_tab_is_absent() {
        let (fixture, plan_id, task_id, lane_path) = socket_bounce_fixture(false);
        dispatch_bounce(&fixture, &plan_id, &task_id).unwrap();

        let task = fixture.service.plan_store.show_task(&task_id).unwrap().task;
        assert_eq!(task.status, crate::plan::TaskStatus::Pending);
        assert_eq!(task.lane.worktree_path.as_deref(), Some(lane_path.as_str()));
        assert!(task.execution_attempts[0].closed_at.is_some());
        assert_eq!(
            task.execution_attempts[0].closure_reason.as_deref(),
            Some("review_bounce_executor_gone")
        );
        assert_eq!(
            task.execution_attempts[0].continuation.state.as_deref(),
            Some("executor_gone")
        );
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_review_keeps_live_unbound_worker_in_review() {
        let (fixture, plan_id, task_id, lane_path) = socket_bounce_fixture(true);
        let error = dispatch_bounce(&fixture, &plan_id, &task_id).unwrap_err();
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("type"))
                .and_then(Value::as_str),
            Some("bounce_continuation_failed")
        );

        let stored_plan = fixture.service.plan_store.get(&plan_id).unwrap();
        let task = stored_plan
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(task.status, crate::plan::TaskStatus::InReview);
        assert_eq!(
            task.status_label.as_deref(),
            Some("bounce_continuation_failed")
        );
        assert_eq!(task.execution_attempts[0].closed_at, None);
        assert_eq!(task.lane.worktree_path.as_deref(), Some(lane_path.as_str()));
        let occupancy =
            super::super::plan_task_dispatch::lane_safety::preflight_checkout_occupancy(
                &stored_plan,
                "retry-task",
                &lane_path,
                crate::service::LaneProtection::Enforced,
                &std::collections::HashMap::new(),
            )
            .expect_err("the unclosed Worker attempt continues to occupy its lane");
        assert_eq!(
            occupancy
                .data
                .as_ref()
                .and_then(|data| data.get("type"))
                .and_then(Value::as_str),
            Some("checkout_occupied")
        );
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_review_does_not_reopen_closed_attempt_while_executor_tab_is_present() {
        let (fixture, plan_id, task_id, lane_path) = socket_bounce_fixture(true);
        let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.execution_attempts[0].closed_at = Some(5);
        fixture.service.plan_store.save(&mut plan, 5).unwrap();

        let error = dispatch_bounce(&fixture, &plan_id, &task_id).unwrap_err();
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("type"))
                .and_then(Value::as_str),
            Some("bounce_continuation_failed")
        );
        let task = fixture.service.plan_store.show_task(&task_id).unwrap().task;
        assert_eq!(task.status, crate::plan::TaskStatus::InReview);
        assert_eq!(task.lane.worktree_path.as_deref(), Some(lane_path.as_str()));
        assert_eq!(task.execution_attempts[0].closed_at, Some(5));
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_review_keeps_missing_remote_worker_attempt_open() {
        let (fixture, plan_id, task_id, _) = socket_bounce_fixture(false);
        let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.execution_attempts[0].mesh_peer = Some("mesh-peer".into());
        fixture.service.plan_store.save(&mut plan, 5).unwrap();

        let error = dispatch_bounce(&fixture, &plan_id, &task_id).unwrap_err();
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("type"))
                .and_then(Value::as_str),
            Some("bounce_continuation_failed")
        );
        let task = fixture.service.plan_store.show_task(&task_id).unwrap().task;
        assert_eq!(task.status, crate::plan::TaskStatus::InReview);
        assert_eq!(task.execution_attempts[0].closed_at, None);
        assert_eq!(task.execution_attempts[0].closure_reason, None);
    }
}
