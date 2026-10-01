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
    let confirmed_human = has_human_confirmation(caller.tab_id.as_deref(), params);
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

fn has_human_confirmation(caller_tab_id: Option<&str>, params: &Option<Value>) -> bool {
    caller_tab_id.is_none()
        && params
            .as_ref()
            .and_then(|value| value.get("human_confirmation"))
            .and_then(Value::as_bool)
            == Some(true)
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

#[cfg(test)]
mod tests {
    use super::{authorize_plan_mutation, has_human_confirmation};
    use serde_json::json;

    #[test]
    fn plan_mutation_human_confirmation_requires_no_tab_identity() {
        let confirmed = Some(json!({ "human_confirmation": true }));
        assert!(has_human_confirmation(None, &confirmed));
        assert!(!has_human_confirmation(Some("planner-tab"), &confirmed));
        assert!(!has_human_confirmation(
            None,
            &Some(json!({ "human_confirmation": false }))
        ));
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn recovered_moved_owner_passes_lifecycle_guard_and_foreign_callers_fail() {
        use crate::multiplexer::state::MultiplexerState;
        use crate::plan::{OrchestratorRef, Plan};
        use crate::service::test_fixture::ServiceFixture;
        use std::sync::Arc;

        let (mut mux, old_owner) = MultiplexerState::new();
        let source = mux.find_workspace_for_tab(&old_owner).unwrap();
        let team = mux
            .resolve_or_mint_team_for_parent(&old_owner)
            .unwrap()
            .team_id;
        let owner = mux.create_tab(&source).unwrap();
        mux.set_orchestrator_role(&owner).unwrap();
        mux.get_tab_mut(&owner).unwrap().team_id = Some(team.clone());
        let foreign = mux.create_tab(&source).unwrap();
        mux.set_orchestrator_role(&foreign).unwrap();
        mux.resolve_or_mint_team_for_parent(&foreign).unwrap();
        for (id, session, nonce) in [
            (&owner, "owner-session", "owner-nonce"),
            (&foreign, "foreign-session", "foreign-nonce"),
        ] {
            let tab = mux.get_tab_mut(id).unwrap();
            tab.agent_session_id = Some(session.into());
            tab.session_binding_nonce = Some(nonce.into());
            tab.reported_session_binding_nonce = Some(nonce.into());
        }
        let fixture = ServiceFixture::new(mux);
        let svc = Arc::new(fixture.service.clone());
        let mut plan = Plan::new(
            "Paused recovery".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&old_owner),
        );
        plan.team_id = Some(team.clone());
        svc.plan_store.save(&mut plan, 1).unwrap();
        svc.plan_store
            .pause_plan(&plan.id, "owner decision", Default::default(), 2)
            .unwrap();
        {
            let mut mux = svc.mux.write();
            mux.close_tab(&old_owner).unwrap();
            crate::service::agent_transition::recover_plan_ownership_for_new_orchestrator(
                &mut mux,
                &svc.plan_store,
                &plan.id,
                OrchestratorRef::from_tab(&owner),
                &team,
                "supported recovery",
                3,
            )
            .unwrap();
            let destination = mux.create_workspace("Moved".into(), None);
            mux.move_tab(&owner, &source, &destination.id).unwrap();
        }
        let before = svc.plan_store.get(&plan.id).unwrap();
        let owner_params = Some(json!({
            "caller_tab_id": owner,
            "session_binding_nonce": "owner-nonce",
        }));
        authorize_plan_mutation(&svc, &plan.id, &owner_params).unwrap();
        for params in [
            Some(json!({ "plan_id": plan.id })),
            Some(json!({ "caller_tab_id": foreign, "session_binding_nonce": "foreign-nonce" })),
            Some(json!({ "caller_tab_id": owner, "session_binding_nonce": "foreign-nonce" })),
        ] {
            let error = authorize_plan_mutation(&svc, &plan.id, &params).unwrap_err();
            assert_eq!(error.code, -32003);
        }
        assert_eq!(svc.plan_store.get(&plan.id).unwrap(), before);
    }
}
