use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::{PlanError, PlanStore};
use crate::service::ServiceState;

use super::{plan_refs, JsonRpcError};

pub(super) fn archive(
    store: &PlanStore,
    svc: &ServiceState,
    app: &AppHandle,
    params: &Option<Value>,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let id = plan_refs::resolve_plan(store, params)?;
    store.archive(&id, now).map_err(plan_rpc_err)?;
    emit_plan_changed(app, &id);
    let mut resp = json!({ "ok": true });
    if let Some(plan) = store.get(&id) {
        let cleanup = crate::service::plan::cleanup_debt::detect_plan_cleanup_debt(&plan, |pane| {
            svc.pane_probe(pane).exists
        });
        push_hints(
            &mut resp,
            crate::service::plan::cleanup_debt::cleanup_debt_hints(&cleanup),
        );
    }
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
