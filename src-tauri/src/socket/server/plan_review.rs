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
                .and_then(|x| x.get("caller_pane_id"))
                .and_then(|v| v.as_str())
                .map(AgentRef::from_pane)
        })
        .unwrap_or_else(|| AgentRef::from_pane("unknown"))
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
    pane_id: &str,
    session_id: &str,
    claimed_nonce: &str,
    live_session_id: Option<&str>,
    configured_nonce: Option<&str>,
    reported_nonce: Option<&str>,
    role: Option<AgentRole>,
) -> Result<(), JsonRpcError> {
    let owner_matches = plan.owner.pane_id.as_deref() == Some(pane_id)
        && plan.owner.session_id.as_deref() == Some(session_id);
    let live_matches = live_session_id == Some(session_id)
        && configured_nonce == Some(claimed_nonce)
        && reported_nonce == Some(claimed_nonce)
        && role == Some(AgentRole::Orchestrator);
    if pane_id.is_empty()
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
    let pane_id = str_field(params, "caller_pane_id");
    let session_id = str_field(params, "session_id");
    let nonce = str_field(params, "session_binding_nonce");
    let plan = svc
        .plan_store
        .get(plan_id)
        .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
    let mux = svc.mux.read();
    let pane = mux
        .get_pane(pane_id)
        .ok_or_else(|| unauthorized("the claimed Orchestrator tab is not live"))?;
    validate_bound_orchestrator_identity(
        &plan,
        pane_id,
        session_id,
        nonce,
        pane.agent_session_id.as_deref(),
        pane.session_binding_nonce.as_deref(),
        pane.reported_session_binding_nonce.as_deref(),
        pane.role,
    )?;
    Ok(AgentRef {
        pane_id: Some(pane.id.clone()),
        session_id: pane.agent_session_id.clone(),
        title: Some(pane.title.clone()),
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
                from.pane_id.as_deref().unwrap_or("unknown"),
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
            let result = store
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
            svc.record_kpi_review_verdict(
                from.pane_id.as_deref().unwrap_or("unknown"),
                &plan_id,
                &task_id,
                crate::agent::kpi_ledger::KpiEventKind::Bounce,
                result.review_round,
                note(p).len(),
            );
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

    fn owned_plan() -> Plan {
        Plan::new(
            "review auth".into(),
            "/repo".into(),
            OrchestratorRef {
                pane_id: Some("owner".into()),
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
}
