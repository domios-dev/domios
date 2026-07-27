use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::{PlanMode, PlanningAuthor, PlanningState};
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "plan.mode.set" | "plan.planning.set" | "plan.pause" | "plan.resume"
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
            let mode = get_string_param(params, "mode")?;
            let mode = serde_json::from_value::<PlanMode>(Value::String(mode))
                .map_err(|_| plan_rpc_err(PlanError::BadMode))?;
            let note = optional_string(params, "note");
            // planning_state (which mirrors mode). Execution/complete are lifecycle.
            match mode {
                PlanMode::Approved => svc
                    .plan_store
                    .set_planning_state(
                        &id,
                        PlanningState::Approved,
                        PlanningAuthor::User,
                        note.as_deref(),
                        epoch_ms(),
                    )
                    .map_err(plan_rpc_err)?,
                PlanMode::Planning => svc
                    .plan_store
                    .set_planning_state(
                        &id,
                        PlanningState::StructureReady,
                        PlanningAuthor::User,
                        note.as_deref(),
                        epoch_ms(),
                    )
                    .map_err(plan_rpc_err)?,
                PlanMode::Execution | PlanMode::Paused | PlanMode::Complete => svc
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
            let reason = get_string_param(params, "reason")?;
            let from = params
                .as_ref()
                .and_then(|p| p.get("from"))
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let stride_id = svc
                .plan_store
                .pause_plan(&id, &reason, from, epoch_ms())
                .map_err(plan_rpc_err)?;
            svc.reconcile_time_reminder_eligibility();
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "stride_id": stride_id }))
        }
        "plan.resume" => {
            let id = super::plan_refs::resolve_plan(&svc.plan_store, params)?;
            let mode = svc
                .resume_team_plan(&id, epoch_ms())
                .map_err(super::team::team_plan_rpc_err)?;
            Ok(json!({ "ok": true, "mode": mode }))
        }
        "plan.planning.set" => {
            let id = super::plan_refs::resolve_plan(&svc.plan_store, params)?;
            let state = get_string_param(params, "state")?;
            let state =
                PlanningState::parse(&state).ok_or_else(|| plan_rpc_err(PlanError::BadMode))?;
            let by = match optional_string(params, "by_tab") {
                Some(tab) => PlanningAuthor::Planner { tab_id: tab },
                None => PlanningAuthor::User,
            };
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
        code: -32603,
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}
