//! `orchestration.*` JSON-RPC handlers — paired Orchestrator+Planner creation
//! (task_7e5f9354), the CLI/socket twin of the REST + Tauri pair-creation
//! endpoints. All three transports call the same
//! `ServiceContext::create_orchestration_pair`.

use serde_json::Value;
use tauri::AppHandle;

use crate::service::{OrchestrationPairParams, ServiceState};

use super::JsonRpcError;

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    _app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "orchestration.new" => {
            let p = params.as_ref().ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let req: OrchestrationPairParams =
                serde_json::from_value(p.clone()).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: format!("invalid params: {e}"),
                    data: None,
                })?;
            crate::service::spawn::validate_agent_model(
                &svc.agent_registry,
                &req.orchestrator_agent,
                req.orchestrator_model.as_deref(),
            )
            .map_err(super::model_errors::invalid_model_rpc_error)?;
            crate::service::spawn::validate_agent_model(
                &svc.agent_registry,
                &req.planner_agent,
                req.planner_model.as_deref(),
            )
            .map_err(super::model_errors::invalid_model_rpc_error)?;

            let svc_ref = &**svc;
            let result = tauri::async_runtime::block_on(svc_ref.create_orchestration_pair(req))
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;

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
