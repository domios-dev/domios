use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::{PlanError, PlanIncident, PlanStore};
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

/// Archive status and its evidence commit as ONE store write, so a failure can
/// never leave a durable incident claiming an archive that did not happen.
pub(super) fn archive_with_evidence(
    store: &PlanStore,
    plan_id: &str,
    now: u64,
) -> Result<Option<SafetyNudge>, PlanError> {
    let nudge = store.get(plan_id).and_then(|plan| retrospective_nudge(&plan));
    let incident = nudge.as_ref().map(|nudge| PlanIncident {
        kind: SAFETY_INCIDENT_KIND.to_string(),
        text: safety_incident_text(plan_id, nudge.id, &nudge.reason),
        from: crate::plan::AgentRef::from_tab("archive"),
    });
    store.archive_with_incident(plan_id, incident, now)?;
    Ok(nudge)
}

pub(super) fn archive(
    store: &PlanStore,
    svc: &ServiceState,
    app: &AppHandle,
    params: &Option<Value>,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let id = plan_refs::resolve_plan(store, params)?;
    let nudge = archive_with_evidence(store, &id, now).map_err(plan_rpc_err)?;
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
