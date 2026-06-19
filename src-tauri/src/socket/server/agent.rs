//! `agent.*` JSON-RPC handlers.

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "agent.spawn" => {
            let p = params.as_ref().ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
            })?;
            let spawn_params =
                crate::service::SpawnAgentParams::from_json(p).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?;

            let svc_ref = &**svc;
            let result = svc_ref
                .spawn_agent_sync(spawn_params)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })?;
            {
                let h = app.clone();
                std::thread::spawn(move || {
                    let _ = h.emit("mux-state-changed", ());
                });
            }

            let mut resp = serde_json::json!({
                "pane_id": result.pane_id,
                "tab_id": result.tab_id,
                "agent": result.agent,
                "title": result.title,
                "role": result.role,
            });
            if let Some(ref warning) = result.warning {
                resp["warning"] = serde_json::json!(warning);
            }
            Ok(resp)
        }

        "agent.report" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let message = get_string_param(params, "message")?;
            // Optional structured fields (status/branch/sha/...) ride alongside
            // the freeform message; absent keys deserialize to None.
            let details = params
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
                .unwrap_or_default();
            let svc_ref = &**svc;
            let result = svc_ref
                .submit_report(&pane_id, &message, details)
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })?;
            Ok(serde_json::json!({
                "ok": true,
                "report_id": result.report_id,
            }))
        }

        "agent.reports" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let clear = params
                .as_ref()
                .and_then(|p| p.get("clear"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let svc_ref = &**svc;
            let reports = if clear {
                svc_ref.get_and_clear_reports(&pane_id)
            } else {
                svc_ref.get_reports(&pane_id)
            };
            serde_json::to_value(&reports).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
            })
        }

        "agent.ledger" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let svc_ref = &**svc;
            let rows = svc_ref.get_ledger(&pane_id);
            serde_json::to_value(&rows).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
            })
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}
