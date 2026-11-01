use serde_json::{json, Value};

use codeterm_service_core::plan::AgentRef;
use codeterm_service_core::service::plan::PlanError;
use codeterm_service_core::service::ServiceState;

use super::JsonRpcError;

pub(super) fn handles(method: &str) -> bool {
    matches!(method, "plan.task.close")
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let now = epoch_ms();
    match method {
        "plan.task.close" => {
            let (id, tid) = super::plan_refs::resolve_task(store, params)?;
            let result = p
                .and_then(|value| value.get("result"))
                .and_then(Value::as_str);
            let field = |key| {
                p.and_then(|body| body.get(key))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let caller = codeterm_service_core::service::AuthenticatedPlanCaller {
                tab_id: field("caller_tab_id"),
                session_id: field("session_id"),
                binding_nonce: field("session_binding_nonce"),
            };
            svc.authorize_task_completion(&id, &caller, result)
                .map_err(|message| JsonRpcError {
                    code: -32003,
                    message,
                    data: None,
                })?;
            let from = AgentRef {
                tab_id: caller.tab_id,
                session_id: caller.session_id,
                title: None,
            };
            store
                .close_task(&id, &tid, result, from, now)
                .map_err(plan_rpc_err)?;
            svc.reconcile_time_reminder_eligibility();
            emit_plan_changed(svc, &id);
            let mut resp = json!({ "ok": true });
            if let Some(plan) = store.get(&id) {
                let outcome = svc.cleanup_closed_task_lane(&plan, &tid, now);
                resp["cleanup"] = json!({ "status": outcome.status(), "reason": outcome.reason() });
                if matches!(
                    &outcome,
                    codeterm_service_core::service::plan::lane_cleanup::LaneCleanupOutcome::Cleaned
                        | codeterm_service_core::service::plan::lane_cleanup::LaneCleanupOutcome::Partial(_)
                ) {
                    emit_plan_changed(svc, &id);
                }
            }
            // Re-read so hints reflect any cleanup that just ran.
            if let Some(plan) = store.get(&id) {
                let cleanup =
                    codeterm_service_core::service::plan::cleanup_debt::detect_plan_cleanup_debt(
                        &plan,
                        |tab_id| svc.tab_probe(tab_id).exists,
                    );
                let hints = codeterm_service_core::service::plan::cleanup_debt::cleanup_debt_hints(
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

fn emit_plan_changed(svc: &ServiceState, plan_id: &str) {
    svc.ui.emit("plan-changed", json!({ "plan_id": plan_id }));
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

#[cfg(test)]
mod tests;
