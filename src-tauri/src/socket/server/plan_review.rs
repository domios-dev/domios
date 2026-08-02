use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::multiplexer::types::AgentRole;
use crate::plan::{AgentRef, Plan, ReviewCapacityClass};
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::JsonRpcError;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
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

fn validate_bound_orchestrator_identity(
    plan: &Plan,
    tab_id: &str,
    session_id: &str,
    claimed_nonce: &str,
    live_session_id: Option<&str>,
    configured_nonce: Option<&str>,
    reported_nonce: Option<&str>,
    role: Option<AgentRole>,
) -> Result<(), JsonRpcError> {
    let owner_matches = plan.owner.tab_id.as_deref() == Some(tab_id)
        && plan.owner.session_id.as_deref() == Some(session_id);
    let live_matches = live_session_id == Some(session_id)
        && configured_nonce == Some(claimed_nonce)
        && reported_nonce == Some(claimed_nonce)
        && role == Some(AgentRole::Orchestrator);
    if tab_id.is_empty()
        || session_id.is_empty()
        || claimed_nonce.is_empty()
        || !owner_matches
        || !live_matches
    {
        return Err(unauthorized(
            "the live bound Plan Orchestrator session is required",
        ));
    }
    Ok(())
}

fn authorized_orchestrator_actor(
    svc: &ServiceState,
    plan_id: &str,
    params: Option<&Value>,
) -> Result<AgentRef, JsonRpcError> {
    let tab_id = str_field(params, "caller_tab_id");
    let session_id = str_field(params, "session_id");
    let nonce = str_field(params, "session_binding_nonce");
    let plan = svc
        .plan_store
        .get(plan_id)
        .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
    let mux = svc.mux.read();
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| unauthorized("the claimed Orchestrator tab is not live"))?;
    validate_bound_orchestrator_identity(
        &plan,
        tab_id,
        session_id,
        nonce,
        tab.agent_session_id.as_deref(),
        tab.session_binding_nonce.as_deref(),
        tab.reported_session_binding_nonce.as_deref(),
        tab.role,
    )?;
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
        "plan.review.accept" => {
            let (plan_id, task_id) = super::plan_refs::resolve_task(store, params)?;
            let residual_risk = p
                .and_then(|x| x.get("residual_risk"))
                .and_then(|v| v.as_str());
            let from = actor(p);
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
            let from = actor(p);
            let mut result = store
                .bounce_review(
                    &plan_id,
                    &task_id,
                    note(p),
                    bool_field(p, "changed_executable_surfaces"),
                    bool_field(p, "distinct_oracle"),
                    from.clone(),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &plan_id);
            if let Some(continuation) = result.continuation.as_mut() {
                let claimed = store
                    .claim_review_continuation_delivery(
                        &plan_id,
                        &task_id,
                        &continuation.attempt_id,
                        continuation.epoch,
                        now,
                    )
                    .map_err(plan_rpc_err)?;
                if claimed {
                    svc.record_kpi_review_verdict(
                        from.tab_id.as_deref().unwrap_or("unknown"),
                        &plan_id,
                        &task_id,
                        crate::agent::kpi_ledger::KpiEventKind::Bounce,
                        result.review_round,
                        note(p).len(),
                    );
                    let binding_nonce = {
                        let mux = svc.mux.read();
                        mux.get_tab(&continuation.executor_tab_id).and_then(|tab| {
                            (tab.agent_session_id.as_deref()
                                == Some(continuation.session_id.as_str())
                                && tab.session_binding_nonce.is_some()
                                && tab.session_binding_nonce == tab.reported_session_binding_nonce)
                                .then(|| tab.session_binding_nonce.clone())
                                .flatten()
                        })
                    };
                    let delivery = binding_nonce.as_deref().ok_or_else(|| {
                        "authenticated Worker continuation binding is unavailable".to_string()
                    }).and_then(|nonce| {
                        let envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder(
                            "review-bounce",
                        )
                        .id(Some(format!(
                            "{}:{}:{}",
                            continuation.attempt_id,
                            continuation.review_round,
                            continuation.epoch
                        )));
                        crate::service::messenger::Messenger::deliver_planner_session(
                            svc,
                            &continuation.executor_tab_id,
                            &continuation.session_id,
                            nonce,
                            envelope,
                            note(p),
                            format!(
                                "review-bounce:{}:{}:{}:{}",
                                plan_id,
                                task_id,
                                continuation.attempt_id,
                                continuation.review_round
                            ),
                        )
                        .map(|_| ())
                    });
                    let delivered = delivery.is_ok();
                    store
                        .finish_review_continuation(
                            &plan_id,
                            &task_id,
                            &continuation.attempt_id,
                            continuation.epoch,
                            delivered,
                            epoch_ms(),
                        )
                        .map_err(plan_rpc_err)?;
                    emit_plan_changed(app, &plan_id);
                    if let Err(error) = delivery {
                        return Err(JsonRpcError {
                            code: -32603,
                            message: format!(
                                "ORCHESTRATION PRODUCT INCIDENT: review bounce continuation failed for {plan_id}/{task_id}/{}: {error}",
                                continuation.attempt_id
                            ),
                            data: Some(json!({
                                "type": "bounce_continuation_failed",
                                "plan_id": plan_id,
                                "task_id": task_id,
                                "attempt_id": continuation.attempt_id,
                                "review_round": continuation.review_round,
                                "error": error,
                            })),
                        });
                    }
                    continuation.state = "continued".into();
                }
            }
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
        store.assign_task(&plan.id, &task_id, "worker", None, 3).unwrap();
        let mut attempt_id = String::new();
        store
            .with_plan(&plan.id, 4, |plan| {
                let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
                task.status = crate::plan::TaskStatus::InReview;
                let attempt = task.execution_attempts.last_mut().unwrap();
                attempt_id = attempt.id.clone();
                attempt.session_id = Some("worker-session".into());
                attempt.acknowledged_at = Some(3);
                attempt.reported_at = Some(4);
                attempt.report = Some(Default::default());
                Ok(())
            })
            .unwrap();
        (dir, store, plan.id, task_id, attempt_id)
    }

    fn owned_plan() -> Plan {
        Plan::new(
            "review auth".into(),
            "/repo".into(),
            OrchestratorRef {
                tab_id: Some("owner".into()),
                session_id: Some("session".into()),
                title: Some("Owner".into()),
            },
        )
    }

    #[test]
    fn review_mutation_auth_accepts_only_the_live_bound_orchestrator_session() {
        let plan = owned_plan();
        validate_bound_orchestrator_identity(
            &plan,
            "owner",
            "session",
            "nonce",
            Some("session"),
            Some("nonce"),
            Some("nonce"),
            Some(AgentRole::Orchestrator),
        )
        .unwrap();

        for rejected in [
            validate_bound_orchestrator_identity(
                &plan,
                "worker",
                "session",
                "nonce",
                Some("session"),
                Some("nonce"),
                Some("nonce"),
                Some(AgentRole::Worker),
            ),
            validate_bound_orchestrator_identity(
                &plan,
                "owner",
                "foreign",
                "nonce",
                Some("foreign"),
                Some("nonce"),
                Some("nonce"),
                Some(AgentRole::Orchestrator),
            ),
            validate_bound_orchestrator_identity(
                &plan,
                "owner",
                "session",
                "forged",
                Some("session"),
                Some("nonce"),
                Some("nonce"),
                Some(AgentRole::Orchestrator),
            ),
        ] {
            assert_eq!(rejected.unwrap_err().code, -32003);
        }
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
            .unwrap());
        assert!(!store
            .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 7)
            .unwrap());
        assert!(store
            .finish_review_continuation(&plan_id, &task_id, &attempt_id, 1, true, 8)
            .unwrap());
        let after = store.show_task(&task_id).unwrap().task;
        assert_eq!(after.status, crate::plan::TaskStatus::InProgress);
        assert_eq!(after.execution_attempts.len(), before.execution_attempts.len());
        assert_eq!(after.execution_attempts[0].acknowledged_at, Some(3));
        assert_eq!(after.execution_attempts[0].continuation.delivered_at, Some(8));
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
            .unwrap());
        assert!(store
            .finish_review_continuation(&plan_id, &task_id, &attempt_id, 1, false, 7)
            .unwrap());
        let failed = store.show_task(&task_id).unwrap().task;
        assert_eq!(failed.status, crate::plan::TaskStatus::InReview);
        assert_eq!(failed.status_label.as_deref(), Some("bounce_continuation_failed"));
        assert_eq!(
            failed.execution_attempts[0].continuation.state.as_deref(),
            Some("bounce_continuation_failed")
        );
    }

    #[test]
    fn review_bounce_fences_stale_rebound_replacement_and_competition_attempts() {
        let (_dir, store, plan_id, task_id, _attempt_id) = continuation_store();
        store
            .with_plan(&plan_id, 5, |plan| {
                let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
                task.assigned_tab_id = Some("replacement".into());
                Ok(())
            })
            .unwrap();
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
        let first = store
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
        let replay = store
            .bounce_review(
                &plan_id,
                &task_id,
                "repair",
                true,
                true,
                AgentRef::from_tab("reviewer"),
                6,
            )
            .unwrap();
        assert_eq!(first.milestone_id, replay.milestone_id);
        assert_eq!(first.continuation, replay.continuation);
        assert!(store
            .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 7)
            .unwrap());
        assert!(!store
            .claim_review_continuation_delivery(&plan_id, &task_id, &attempt_id, 1, 8)
            .unwrap());
    }
}
