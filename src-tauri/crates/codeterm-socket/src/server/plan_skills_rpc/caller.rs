use serde_json::{json, Value};

use super::{plan_refs, plan_rpc_err, JsonRpcError, PlanStore};
use codeterm_service_core::multiplexer::state::MultiplexerState;

#[derive(PartialEq, Eq)]
pub(crate) struct SkillCaller {
    pub tab_id: String,
    pub session_id: String,
    pub nonce: String,
    pub team_id: Option<String>,
}

pub(super) fn authenticate(
    mux: &MultiplexerState,
    params: &Option<Value>,
) -> Result<Option<SkillCaller>, JsonRpcError> {
    let Some(body) = params else {
        return Ok(None);
    };
    let tab_id = optional_claim(body, "caller_tab_id")?;
    let nonce = optional_claim(body, "session_binding_nonce")?;
    let session_id = optional_claim(body, "caller_session_id")?;
    let claims = [
        "caller_tab_id",
        "session_binding_nonce",
        "caller_session_id",
        "task_id",
        "attempt_id",
    ];
    let (Some(tab_id), Some(nonce)) = (tab_id, nonce) else {
        if claims.iter().any(|key| body.get(key).is_some()) {
            return Err(refused("plan skill caller identity is incomplete"));
        }
        return Ok(None);
    };
    let tab = mux
        .get_tab(tab_id)
        .filter(|tab| {
            tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        })
        .ok_or_else(|| refused("plan skill caller binding is not current"))?;
    let live_session = tab
        .agent_session_id
        .as_deref()
        .filter(|session| !session.trim().is_empty())
        .ok_or_else(|| refused("plan skill caller session is not nonce-proven by a live tab"))?;
    if session_id.is_some_and(|session| session != live_session) {
        return Err(refused("plan skill caller binding is not current"));
    }
    Ok(Some(SkillCaller {
        tab_id: tab_id.into(),
        session_id: live_session.into(),
        nonce: nonce.into(),
        team_id: tab.team_id.clone(),
    }))
}

pub(super) fn optional_claim<'a>(
    body: &'a Value,
    key: &str,
) -> Result<Option<&'a str>, JsonRpcError> {
    body.get(key)
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| refused("plan skill target claim is invalid"))
        })
        .transpose()
}

pub(super) fn resolve_plan(
    store: &PlanStore,
    params: &Option<Value>,
    caller: Option<&SkillCaller>,
) -> Result<String, JsonRpcError> {
    if params
        .as_ref()
        .and_then(|body| body.get("plan_id"))
        .is_some()
    {
        return plan_refs::resolve_plan(store, params);
    }
    if let Some(team_id) = caller.and_then(|caller| caller.team_id.as_deref()) {
        return store
            .active_plan_for_team(team_id)
            .map(|(id, _)| id)
            .ok_or_else(|| refused("caller Team has no active plan; pass the plan id explicitly"));
    }
    plan_refs::resolve_plan(store, params)
}

pub(super) fn requires_current_receipt(
    store: &PlanStore,
    plan_id: &str,
    caller: &SkillCaller,
) -> bool {
    if caller
        .team_id
        .as_deref()
        .and_then(|team| store.active_plan_for_team(team))
        .is_some_and(|(active, _)| active != plan_id)
    {
        return false;
    }
    store.get(plan_id).is_some_and(|plan| {
        plan.tasks.iter().any(|task| {
            !task.status.is_terminal()
                && (task.assigned_tab_id.as_deref() == Some(&caller.tab_id)
                    || task.execution_attempts.iter().any(|attempt| {
                        attempt.executor_tab_id == caller.tab_id
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    }))
        })
    })
}

fn refused(message: &str) -> JsonRpcError {
    plan_rpc_err(codeterm_service_core::service::plan::PlanError::Io(
        message.into(),
    ))
}

pub(crate) fn ack_skill_brief(
    store: &PlanStore,
    mux: &MultiplexerState,
    params: &Option<Value>,
    acknowledgement: &codeterm_service_core::service::plan::task_attempts::TaskAttemptAcknowledgement,
    now_ms: u64,
) -> Result<Value, JsonRpcError> {
    let mut identity = params.clone().unwrap_or_else(|| json!({}));
    identity["caller_tab_id"] = identity["tab_id"].clone();
    let attempt_bound = acknowledgement
        .attempt
        .session_id
        .as_deref()
        .is_some_and(|session| !session.trim().is_empty())
        && acknowledgement
            .attempt
            .session_binding_nonce
            .as_deref()
            .is_some_and(|nonce| !nonce.trim().is_empty());
    if acknowledgement.already_acknowledged && !attempt_bound {
        return Err(attempt_binding_refusal(
            acknowledgement,
            "no retained launch completion",
        ));
    }
    let bound = attempt_bound && identity.get("session_binding_nonce").is_some();
    let manifest = if bound {
        let caller = authenticate(mux, &Some(identity))?
            .ok_or_else(|| refused("plan skill announcement requires a live caller"))?;
        store
            .announce_current_plan_skill_manifest(
                &acknowledgement.plan_id,
                &acknowledgement.task_id,
                &acknowledgement.attempt.id,
                &caller.tab_id,
                &caller.session_id,
                &caller.nonce,
                now_ms,
            )
            .map_err(plan_rpc_err)?
    } else {
        acknowledgement.attempt.plan_skill_manifest.clone()
    };
    Ok(json!({
        "plan_skill_brief": codeterm_service_core::service::plan::task_brief::render_plan_skill_manifest(&acknowledgement.plan_id, &manifest),
        "plan_skill_manifest": manifest,
    }))
}

pub(crate) fn attempt_binding_refusal(
    acknowledgement: &codeterm_service_core::service::plan::task_attempts::TaskAttemptAcknowledgement,
    cause: &str,
) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: format!("ACK is retained, but this attempt has no dispatch-proven session binding: {cause}. Report the plan, task, and attempt to the Orchestrator for launch-proven binding repair."),
        data: Some(json!({
            "type": "plan_skill_attempt_binding_missing",
            "plan_id": acknowledgement.plan_id,
            "task_id": acknowledgement.task_id,
            "attempt_id": acknowledgement.attempt.id,
            "acknowledged_at": acknowledgement.attempt.acknowledged_at,
            "recovery": "orchestrator_launch_proven_binding_repair",
        })),
    }
}
