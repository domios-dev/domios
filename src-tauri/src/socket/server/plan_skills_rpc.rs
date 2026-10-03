use serde_json::{json, Value};
use sha2::Digest;

use super::plan::plan_rpc_err;
use super::{get_string_param, plan_refs, JsonRpcError};
use crate::plan::PlannerRef;
use crate::service::plan::plan_skills::{
    create_plan_skill, list_plan_skills, show_plan_skill, NewPlanSkill,
};
use crate::service::plan::PlanStore;
use crate::service::ServiceState;

pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "plan.skills.list" | "plan.skills.show" | "plan.skills.new"
    )
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
    now_ms: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let result = dispatch_store(method, params, store)?;
    if method != "plan.skills.show" {
        return Ok(result);
    }
    let plan_id = plan_refs::resolve_plan(store, params)?;
    let Some(body) = params.as_ref() else {
        return Ok(result);
    };
    let supplied_session_id = body.get("caller_session_id").and_then(Value::as_str);
    let identity = [
        body.get("caller_tab_id").and_then(Value::as_str),
        body.get("session_binding_nonce").and_then(Value::as_str),
    ];
    let [Some(tab_id), Some(nonce)] = identity else {
        if identity.iter().any(Option::is_some)
            || supplied_session_id.is_some()
            || body.get("task_id").is_some()
            || body.get("attempt_id").is_some()
        {
            return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
                "plan skill caller identity is incomplete".into(),
            )));
        }
        return Ok(result);
    };
    let session_id = match supplied_session_id {
        Some(session_id) => session_id.to_string(),
        None => nonce_proven_session_id(svc, tab_id, nonce)?,
    };
    let live = svc.mux.read().get_tab(tab_id).map(|tab| {
        tab.agent_session_id.as_deref() == Some(session_id.as_str())
            && tab.session_binding_nonce.as_deref() == Some(nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
    }) == Some(true);
    if !live {
        return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
            "plan skill caller binding is not current".into(),
        )));
    }
    // Owner/Planner reads their own plan without a task binding or ledger entry.
    if plan_caller_is_owner_or_planner(store, &plan_id, tab_id) {
        return Ok(result);
    }
    let task_id = resolve_skill_task_id(
        store,
        &plan_id,
        tab_id,
        body.get("task_id").and_then(Value::as_str),
    )
    .map_err(plan_rpc_err)?;
    let skill = result.get("skill").ok_or_else(|| JsonRpcError {
        code: -32603,
        message: "plan skill response is missing skill".into(),
        data: None,
    })?;
    let slug = skill
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let body = skill
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let digest = format!("sha256:{:x}", sha2::Sha256::digest(body.as_bytes()));
    let attempt_id = params
        .as_ref()
        .and_then(|params| params.get("attempt_id"))
        .and_then(Value::as_str);
    store
        .record_plan_skill_load(
            &plan_id,
            &task_id,
            tab_id,
            &session_id,
            nonce,
            attempt_id,
            slug,
            &digest,
            now_ms,
        )
        .map_err(plan_rpc_err)?;
    Ok(result)
}

/// Checked only after `live` proves the caller genuinely owns `tab_id`.
fn plan_caller_is_owner_or_planner(store: &PlanStore, plan_id: &str, tab_id: &str) -> bool {
    let Some(plan) = store.get(plan_id) else {
        return false;
    };
    if plan.owner.tab_id.as_deref() == Some(tab_id) {
        return true;
    }
    matches!(
        &plan.planner,
        Some(PlannerRef::Tab { tab_id: bound, .. }) if bound == tab_id
    )
}

// A task tab cannot export its session id before PTY spawn; only a nonce that
// is both issued to the tab and echoed back proves the session that owns it.
fn nonce_proven_session_id(
    svc: &ServiceState,
    tab_id: &str,
    nonce: &str,
) -> Result<String, JsonRpcError> {
    svc.mux
        .read()
        .get_tab(tab_id)
        .filter(|tab| {
            tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        })
        .and_then(|tab| tab.agent_session_id.clone())
        .ok_or_else(|| {
            plan_rpc_err(crate::service::plan::PlanError::Io(
                "plan skill caller session is not nonce-proven by a live tab".into(),
            ))
        })
}

fn resolve_skill_task_id(
    store: &PlanStore,
    plan_id: &str,
    tab_id: &str,
    supplied_task_id: Option<&str>,
) -> Result<String, crate::service::plan::PlanError> {
    supplied_task_id
        .map(str::to_string)
        .or_else(|| {
            store
                .task_binding_for_tab(tab_id)
                .filter(|(bound_plan_id, _)| bound_plan_id == plan_id)
                .map(|(_, task_id)| task_id)
        })
        .ok_or_else(|| {
            crate::service::plan::PlanError::Io(
                "plan skill caller has no current task binding".into(),
            )
        })
}

fn dispatch_store(
    method: &str,
    params: &Option<Value>,
    store: &PlanStore,
) -> Result<Value, JsonRpcError> {
    let plan_id = plan_refs::resolve_plan(store, params)?;
    match method {
        "plan.skills.list" => {
            let skills = list_plan_skills(store, &plan_id).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "skills": skills }))
        }
        "plan.skills.show" => {
            let slug = get_string_param(params, "slug")?;
            let skill = show_plan_skill(store, &plan_id, &slug).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "skill": skill }))
        }
        "plan.skills.new" => {
            let body = params.as_ref().ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let request = NewPlanSkill {
                slug: get_string_param(params, "slug")?,
                title: optional_string(body, "title"),
                area: optional_string(body, "area"),
                check: optional_string(body, "check"),
                rationale: optional_string(body, "rationale"),
            };
            let skill = create_plan_skill(store, &plan_id, request).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "skill": skill }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests;
