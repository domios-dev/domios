use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::plan::PlanError;
use crate::service::{OrchestratorLearningError, ServiceState};

use super::{get_string_param, JsonRpcError};

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
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

fn learning_error(error: OrchestratorLearningError) -> JsonRpcError {
    match error {
        OrchestratorLearningError::Authority(message) => JsonRpcError {
            code: super::error_codes::PLAN_ORCHESTRATOR_REQUIRED,
            message,
            data: Some(json!({ "type": "plan_orchestrator_required" })),
        },
        OrchestratorLearningError::Store(error) => plan_rpc_err(error),
    }
}

fn emit_plan_changed(app: &AppHandle, plan_id: &str) {
    crate::api::events::emit_everywhere(app, "plan-changed", json!({ "plan_id": plan_id }));
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    let body = params.as_ref().ok_or(JsonRpcError {
        code: -32602,
        message: "params required".to_string(),
        data: None,
    })?;
    match method {
        "plan.heartbeat.learn" => {
            let text = get_string_param(params, "text")?;
            let plan_id = super::plan_refs::resolve_plan(&svc.plan_store, params)?;
            let caller = crate::service::AuthenticatedPlanCaller {
                tab_id: body
                    .get("caller_tab_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                session_id: body
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                binding_nonce: body
                    .get("session_binding_nonce")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
            let result = svc
                .add_orchestrator_heartbeat_learning(
                    &plan_id,
                    &caller,
                    &text,
                    body.get("tag").and_then(|v| v.as_str()),
                    epoch_ms(),
                )
                .map_err(learning_error)?;
            emit_plan_changed(app, &result.plan_id);
            if let Some(tab_id) = body.get("tab_id").and_then(|v| v.as_str()) {
                svc.maybe_refresh_after_reflection(
                    tab_id,
                    &result.plan_id,
                    body.get("tag").and_then(|v| v.as_str()),
                );
            }
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        "plan.heartbeat.show" => {
            let result = svc
                .plan_store
                .show_heartbeat(
                    body.get("plan_id").and_then(|v| v.as_str()),
                    body.get("tab_id").and_then(|v| v.as_str()),
                    body.get("session_id").and_then(|v| v.as_str()),
                )
                .map_err(plan_rpc_err)?;
            serde_json::to_value(result).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
                data: None,
            })
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::learning_error;
    use crate::service::OrchestratorLearningError;

    #[test]
    fn orchestrator_learning_refusal_uses_the_authority_error_code() {
        let error = learning_error(OrchestratorLearningError::Authority("refused".into()));

        assert_eq!(
            error.code,
            super::super::error_codes::PLAN_ORCHESTRATOR_REQUIRED
        );
        assert_eq!(error.data.unwrap()["type"], "plan_orchestrator_required");
    }
}
