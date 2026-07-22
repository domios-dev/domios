use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::AgentRef;
use crate::service::plan::PlanError;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "plan.task.pause" | "plan.task.resume" | "plan.task.close"
    )
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let now = epoch_ms();
    match method {
        "plan.task.pause" => {
            let (id, tid) = super::plan_refs::resolve_task(store, params)?;
            let reason = get_string_param(params, "reason")?;
            let milestone_id = store
                .pause_task(&id, &tid, &reason, from_param(p), now)
                .map_err(plan_rpc_err)?;
            svc.reconcile_time_reminder_eligibility();
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id, "task_id": tid, "milestone_id": milestone_id }))
        }
        "plan.task.resume" => {
            let (id, tid) = super::plan_refs::resolve_task(store, params)?;
            let milestone_id = store
                .resume_task(&id, &tid, from_param(p), now)
                .map_err(plan_rpc_err)?;
            svc.reconcile_time_reminder_eligibility();
            emit_plan_changed(app, &id);
            Ok(json!({ "ok": true, "plan_id": id, "task_id": tid, "milestone_id": milestone_id }))
        }
        "plan.task.close" => {
            let (id, tid) = super::plan_refs::resolve_task(store, params)?;
            store
                .close_task(
                    &id,
                    &tid,
                    p.and_then(|x| x.get("result")).and_then(|v| v.as_str()),
                    from_param(p),
                    now,
                )
                .map_err(plan_rpc_err)?;
            svc.reconcile_time_reminder_eligibility();
            emit_plan_changed(app, &id);
            let mut resp = json!({ "ok": true });
            if let Some(plan) = store.get(&id) {
                let outcome = crate::service::plan::lane_cleanup::try_cleanup_task_lane(
                    store,
                    &plan,
                    &tid,
                    |pane_id| svc.pane_probe(pane_id).exists,
                    now,
                );
                resp["cleanup"] = json!({ "status": outcome.status(), "reason": outcome.reason() });
                if matches!(
                    outcome,
                    crate::service::plan::lane_cleanup::LaneCleanupOutcome::Cleaned
                ) {
                    emit_plan_changed(app, &id);
                }
            }
            // Re-read so hints reflect any cleanup that just ran.
            if let Some(plan) = store.get(&id) {
                let cleanup = crate::service::plan::cleanup_debt::detect_plan_cleanup_debt(
                    &plan,
                    |pane_id| svc.pane_probe(pane_id).exists,
                );
                let hints = crate::service::plan::cleanup_debt::cleanup_debt_hints(
                    &cleanup
                        .into_iter()
                        .filter(|item| item.task_id == tid)
                        .collect::<Vec<_>>(),
                );
                push_hints(&mut resp, hints);
            }
            Ok(resp)
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn from_param(p: Option<&Value>) -> AgentRef {
    p.and_then(|x| x.get("from"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| AgentRef::from_pane("unknown"))
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

fn push_hints(value: &mut Value, hints: Vec<String>) {
    let hints: Vec<String> = hints
        .into_iter()
        .filter(|hint| !hint.trim().is_empty())
        .collect();
    if hints.is_empty() {
        return;
    }
    value["_hints"] = json!(hints);
}
