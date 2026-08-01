//! `orchestrator.*` JSON-RPC handlers. Currently `orchestrator.refresh` — the
//! manual trigger for the daemon-side S1..S5 refresh cycle. The CLI twin is
//! `codeterm orchestrator refresh`.

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::service::{RefreshOutcome, RefreshRequest, RefreshTrigger, ServiceState};

use super::JsonRpcError;

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    _app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "orchestrator.refresh" => {
            let p = params.as_ref().ok_or_else(|| invalid("params required"))?;
            let tab_id = p
                .get("tab_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| invalid("tab_id required"))?
                .to_string();
            let plan_id = p
                .get("plan_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| invalid("plan_id required"))?
                .to_string();
            let req = RefreshRequest::from_config(tab_id, plan_id, RefreshTrigger::Manual);
            let outcome = svc.orchestrator_refresh(req);
            let status = match outcome {
                RefreshOutcome::Started => "started",
                RefreshOutcome::Coalesced => "coalesced",
                RefreshOutcome::NotEligible => "not_eligible",
            };
            Ok(json!({ "ok": true, "status": status }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn invalid(msg: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: msg.to_string(),
        data: None,
    }
}
