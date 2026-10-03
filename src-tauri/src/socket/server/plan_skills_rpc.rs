use serde_json::{json, Value};
use sha2::Digest;

use super::plan::plan_rpc_err;
use super::{get_string_param, plan_refs, JsonRpcError};
use crate::service::plan::plan_skills::{
    create_plan_skill, list_plan_skills, show_plan_skill, NewPlanSkill,
};
use crate::service::plan::PlanStore;
use crate::service::ServiceState;

mod caller;
pub(super) use caller::{ack_skill_brief, attempt_binding_refusal};

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
    let caller = {
        let mux = svc.mux.read();
        caller::authenticate(&mux, params)?
    };
    let plan_id = caller::resolve_plan(store, params, caller.as_ref())?;
    let mut resolved = params.clone().unwrap_or_else(|| json!({}));
    resolved["plan_id"] = json!(plan_id);
    let result = dispatch_store(method, &Some(resolved), store)?;
    if method != "plan.skills.show" {
        return Ok(result);
    }
    let Some(caller) = caller else {
        return Ok(result);
    };
    let body = params.as_ref().ok_or_else(|| {
        plan_rpc_err(crate::service::plan::PlanError::Io(
            "params required".into(),
        ))
    })?;
    let task_claim = caller::optional_claim(body, "task_id")?;
    let attempt_claim = caller::optional_claim(body, "attempt_id")?;
    if task_claim.is_none()
        && attempt_claim.is_none()
        && !caller::requires_current_receipt(store, &plan_id, &caller)
    {
        return Ok(result);
    }
    let current = store
        .current_plan_skill_attempt(&caller.tab_id, &caller.session_id, &caller.nonce)
        .map_err(plan_rpc_err)?;
    let Some((bound_plan, task_id, attempt_id)) = current else {
        if task_claim.is_some() || attempt_claim.is_some() {
            return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
                "plan skill caller has no current task binding".into(),
            )));
        }
        return Ok(result);
    };
    if bound_plan != plan_id {
        if task_claim.is_some() || attempt_claim.is_some() {
            return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
                "cross-plan skill reads cannot claim current-task evidence".into(),
            )));
        }
        return Ok(result);
    }
    if task_claim.is_some_and(|id| id != task_id)
        || attempt_claim.is_some_and(|id| id != attempt_id)
    {
        return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
            "plan skill target conflicts with current assignment".into(),
        )));
    }
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
    let mux = svc.mux.read();
    if caller::authenticate(&mux, params)?.as_ref() != Some(&caller) {
        return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
            "plan skill caller binding changed during the read".into(),
        )));
    }
    store
        .record_plan_skill_load(
            &plan_id,
            &task_id,
            &caller.tab_id,
            &caller.session_id,
            &caller.nonce,
            Some(&attempt_id),
            slug,
            &digest,
            now_ms,
        )
        .map_err(plan_rpc_err)?;
    Ok(result)
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
