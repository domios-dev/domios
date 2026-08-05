use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::{PlanError, PlanStore};
use crate::service::ServiceState;

use super::safety_nudge::{
    apply_safety_nudges, safety_incident_text, SafetyNudge, NUDGE_RETROSPECTIVE_UNRECORDED,
    SAFETY_INCIDENT_KIND,
};
use super::{plan_refs, JsonRpcError};

/// Advisory, never a gate. A missing retrospective is worth saying loudly, but
/// refusing here would trap closure exactly as the stale epoch trapped recovery.
pub(super) fn retrospective_nudge(plan: &crate::plan::Plan) -> Option<SafetyNudge> {
    if crate::service::plan::retrospective::has_retrospective(plan) {
        return None;
    }
    Some(SafetyNudge::new(
        NUDGE_RETROSPECTIVE_UNRECORDED,
        format!(
            "plan {} was archived with no retrospective, so what worked and what \
             failed is not recoverable from plan state.",
            plan.id
        ),
        format!(
            "codeterm plan retrospective {} --body-file <pad-relative-path>",
            plan.id
        ),
    ))
}

pub(super) fn archive(
    store: &PlanStore,
    svc: &ServiceState,
    app: &AppHandle,
    params: &Option<Value>,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let id = plan_refs::resolve_plan(store, params)?;
    let nudge = store.get(&id).and_then(|plan| retrospective_nudge(&plan));
    if let Some(nudge) = nudge.as_ref() {
        record_retrospective_incident(store, &id, nudge, now);
    }
    store.archive(&id, now).map_err(plan_rpc_err)?;
    emit_plan_changed(app, &id);
    let mut resp = json!({ "ok": true });
    if let Some(plan) = store.get(&id) {
        let cleanup = crate::service::plan::cleanup_debt::detect_plan_cleanup_debt(&plan, |tab| {
            svc.tab_probe(tab).exists
        });
        push_hints(
            &mut resp,
            crate::service::plan::cleanup_debt::cleanup_debt_hints(&cleanup),
        );
    }
    // After push_hints, which overwrites `_hints`; apply_safety_nudges merges.
    apply_safety_nudges(&mut resp, nudge.as_slice());
    Ok(resp)
}

fn record_retrospective_incident(
    store: &PlanStore,
    plan_id: &str,
    nudge: &SafetyNudge,
    now: u64,
) {
    let text = safety_incident_text(plan_id, nudge.id, &nudge.reason);
    if let Err(error) = store.add_plan_stride(
        plan_id,
        SAFETY_INCIDENT_KIND,
        &text,
        crate::plan::AgentRef::from_tab("archive"),
        now,
    ) {
        log::warn!("[safety] retrospective evidence not recorded for {plan_id}: {error}");
    }
}

pub(super) fn unarchive(
    store: &PlanStore,
    svc: &ServiceState,
    params: &Option<Value>,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let id = plan_refs::resolve_plan(store, params)?;
    svc.unarchive_team_plan(&id, now)
        .map_err(super::team::team_plan_rpc_err)?;
    Ok(json!({ "ok": true }))
}

fn push_hints(value: &mut Value, hints: Vec<String>) {
    let hints: Vec<String> = hints
        .into_iter()
        .filter(|hint| !hint.trim().is_empty())
        .collect();
    if !hints.is_empty() {
        value["_hints"] = json!(hints);
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: e.to_string(),
        data: Some(e.to_json()),
    }
}
