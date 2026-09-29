use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::{PlanMode, PlanningAuthor, PlanningState};
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "plan.mode.set"
            | "plan.planning.set"
            | "plan.pause"
            | "plan.resume"
            | "plan.session.rebind"
    )
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "plan.mode.set" => {
            let id = super::plan_refs::resolve_plan(&svc.plan_store, params)?;
            authorize_plan_mutation(svc, &id, params)?;
            let mode = get_string_param(params, "mode")?;
            let mode = serde_json::from_value::<PlanMode>(Value::String(mode))
                .map_err(|_| plan_rpc_err(PlanError::BadMode))?;
            let note = optional_string(params, "note");
            // planning_state (which mirrors mode). Execution/complete are lifecycle.
            match mode {
                PlanMode::Approved => svc
                    .approve_and_activate_plan(&id, &caller(params), note.as_deref(), epoch_ms())
                    .map_err(authority_error)?,
                PlanMode::Planning => {
                    svc.authorize_plan_orchestrator(&id, &caller(params))
                        .map_err(authority_error)?;
                    svc.plan_store
                        .set_planning_state(
                            &id,
                            PlanningState::StructureReady,
                            PlanningAuthor::User,
                            note.as_deref(),
                            epoch_ms(),
                        )
                        .map_err(plan_rpc_err)?
                }
                PlanMode::Execution => svc
                    .enter_execution_mode(&id, &caller(params), epoch_ms())
                    .map_err(authority_error)?,
                PlanMode::Paused | PlanMode::Complete => svc
                    .plan_store
                    .set_mode(&id, mode, epoch_ms())
                    .map_err(plan_rpc_err)?,
            }
            svc.reconcile_time_reminder_eligibility();
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true }))
        }
        "plan.pause" => {
            let id = super::plan_refs::resolve_plan(&svc.plan_store, params)?;
            authorize_plan_mutation(svc, &id, params)?;
            let reason = get_string_param(params, "reason")?;
            let from = params
                .as_ref()
                .and_then(|p| p.get("from"))
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let outcome = svc
                .pause_plan_operation(&id, &reason, from, epoch_ms())
                .map_err(super::team::team_plan_rpc_err)?;
            let operation_id = outcome.operation.as_ref().map(|op| op.id.clone());
            Ok(json!({
                "ok": true,
                "applied": outcome.applied,
                "stride_id": outcome.stride_id.unwrap_or_default(),
                "operation_id": operation_id,
            }))
        }
        "plan.resume" => {
            let id = super::plan_refs::resolve_plan(&svc.plan_store, params)?;
            authorize_plan_mutation(svc, &id, params)?;
            let mode = svc
                .resume_team_plan(&id, epoch_ms())
                .map_err(super::team::team_plan_rpc_err)?;
            Ok(json!({ "ok": true, "mode": mode }))
        }
        "plan.session.rebind" => {
            let outcome = svc
                .rebind_stale_planning_session(&caller(params))
                .map_err(rebind_error)?;
            Ok(json!({
                "ok": true,
                "tab_id": outcome.tab_id,
                "session_binding_nonce": outcome.nonce,
                "resynced": outcome.resynced,
            }))
        }
        "plan.planning.set" => {
            let id = super::plan_refs::resolve_plan(&svc.plan_store, params)?;
            let state = get_string_param(params, "state")?;
            let state =
                PlanningState::parse(&state).ok_or_else(|| plan_rpc_err(PlanError::BadMode))?;
            if matches!(
                state,
                PlanningState::StructureReady | PlanningState::Approved
            ) {
                return Err(JsonRpcError {
                    code: -32602,
                    message: "structure readiness is derived; the Orchestrator approves and activates with `codeterm plan approve <plan>`".into(),
                    data: Some(json!({ "type": "manual_structure_activation_forbidden" })),
                });
            }
            let by = svc
                .authorize_draft_structure_mutation(&id, &caller(params))
                .map_err(authority_error)?;
            let note = optional_string(params, "note");
            svc.plan_store
                .set_planning_state(&id, state, by, note.as_deref(), epoch_ms())
                .map_err(plan_rpc_err)?;
            svc.reconcile_time_reminder_eligibility();
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "planning_state": state.as_str() }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn caller(params: &Option<Value>) -> crate::service::AuthenticatedPlanCaller {
    let field = |key| optional_string(params, key);
    crate::service::AuthenticatedPlanCaller {
        tab_id: field("caller_tab_id"),
        session_id: field("session_id"),
        binding_nonce: field("session_binding_nonce"),
    }
}

pub(super) fn authorize_plan_mutation(
    svc: &ServiceState,
    plan_id: &str,
    params: &Option<Value>,
) -> Result<(), JsonRpcError> {
    let caller = caller(params);
    let confirmed_human = params
        .as_ref()
        .and_then(|value| value.get("human_confirmation"))
        .and_then(Value::as_bool)
        == Some(true)
        && caller.tab_id.is_none();
    if confirmed_human
        || (caller.tab_id.is_some()
            && matches!(
            svc.classify_planning_caller(plan_id, &caller),
            Ok(crate::plan::PlanningAuthor::User | crate::plan::PlanningAuthor::Planner { .. })
        ))
    {
        return Ok(());
    }
    Err(JsonRpcError {
        code: -32003,
        message: "the caller is not the Plan Orchestrator or Planner".into(),
        data: Some(json!({ "type": "plan_mutation_authorization_required" })),
    })
}

fn authority_error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: super::error_codes::PLAN_ORCHESTRATOR_REQUIRED,
        message,
        data: Some(json!({ "type": "plan_approval_authorization_required" })),
    }
}

fn rebind_error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32003,
        message,
        data: Some(json!({ "type": "plan_binding_rebind_refused" })),
    }
}

fn optional_string(params: &Option<Value>, key: &str) -> Option<String> {
    params
        .as_ref()
        .and_then(|p| p.get(key))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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
