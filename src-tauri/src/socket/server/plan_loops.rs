use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::AgentRef;
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn plan_rpc_err(e: PlanError) -> JsonRpcError {
    let data = e.to_json();
    JsonRpcError {
        code: e.rpc_code(),
        message: e.to_string(),
        data: Some(data),
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}

fn emit_loop_plan_changed(app: &AppHandle, svc: &ServiceState, lid: &str) {
    let store = &svc.plan_store;
    if let Some(plan_id) = store.list_index().iter().find_map(|e| {
        store
            .get(&e.plan_id)
            .filter(|pl| pl.loops.iter().any(|l| l.id == lid))
            .map(|pl| pl.id)
    }) {
        emit_plan_changed(app, &plan_id);
    }
}

pub(super) fn handles(method: &str) -> bool {
    method.starts_with("plan.loop.")
        || method.starts_with("plan.milestone.")
        || method.starts_with("plan.stride.")
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
        "plan.milestone.add" => {
            let (id, tid) = super::plan_refs::resolve_task(store, params)?;
            let kind = get_string_param(params, "kind")?;
            let text = p
                .and_then(|x| x.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let add_kind = p
                .and_then(|x| x.get("add_kind"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let from = p
                .and_then(|x| x.get("from"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| AgentRef::from_tab("unknown"));
            let mid = store
                .add_milestone(&id, &tid, &kind, text, from, add_kind, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "milestone_id": mid }))
        }

        "plan.milestone.update" => {
            let (id, tid, ms_id) = super::plan_refs::resolve_milestone(store, params)?;
            store
                .update_milestone(
                    &id,
                    &tid,
                    &ms_id,
                    p.and_then(|x| x.get("kind")).and_then(|v| v.as_str()),
                    p.and_then(|x| x.get("text")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "milestone_id": ms_id }))
        }

        "plan.milestone.rm" => {
            let (id, tid, ms_id) = super::plan_refs::resolve_milestone(store, params)?;
            store
                .rm_milestone(&id, &tid, &ms_id, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "milestone_id": ms_id }))
        }

        "plan.stride.add" => {
            let id = super::plan_refs::resolve_plan(store, params)?;
            let kind = get_string_param(params, "kind")?;
            let text = p
                .and_then(|x| x.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let from = p
                .and_then(|x| x.get("from"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(|| AgentRef::from_tab("unknown"));
            let mid = store
                .add_plan_stride(&id, &kind, text, from, now)
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "stride_id": mid }))
        }

        "plan.loop.create" => {
            let id = super::plan_refs::resolve_plan(store, params)?;
            let target = get_string_param(params, "target")?;
            let budget = p
                .and_then(|x| x.get("budget"))
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            let lid = store
                .create_loop(
                    &id,
                    &target,
                    budget,
                    p.and_then(|x| x.get("stop_when")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_plan_changed(app, &id);
            Ok(json!({ "loop_id": lid }))
        }

        "plan.loop.set" => {
            let lid = super::plan_refs::resolve_loop(store, params)?;
            store
                .set_loop(
                    &lid,
                    p.and_then(|x| x.get("budget"))
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32),
                    Some(p.and_then(|x| x.get("stop_when")).and_then(|v| v.as_str())),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_loop_plan_changed(app, svc, &lid);
            Ok(json!({ "ok": true, "loop_id": lid }))
        }

        "plan.loop.status" => {
            let lid = super::plan_refs::resolve_loop(store, params)?;
            let spec = store
                .loop_status(&lid)
                .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
            Ok(serde_json::to_value(spec).unwrap_or_default())
        }

        "plan.loop.record" => {
            let lid = super::plan_refs::resolve_loop(store, params)?;
            let note = get_string_param(params, "note")?;
            let result = store
                .record_loop(
                    &lid,
                    &note,
                    p.and_then(|x| x.get("stop"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    p.and_then(|x| x.get("task_id")).and_then(|v| v.as_str()),
                    now,
                )
                .map_err(plan_rpc_err)?;
            emit_loop_plan_changed(app, svc, &lid);
            Ok(serde_json::to_value(result).unwrap_or_default())
        }

        "plan.loop.stop" => {
            let lid = super::plan_refs::resolve_loop(store, params)?;
            store.stop_loop(&lid, now).map_err(plan_rpc_err)?;
            emit_loop_plan_changed(app, svc, &lid);
            Ok(json!({ "ok": true, "loop_id": lid }))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
