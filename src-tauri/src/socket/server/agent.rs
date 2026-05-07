//! `agent.*` JSON-RPC handlers.

use std::time::Duration;

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
            let spawn_params = crate::service::SpawnAgentParams::from_json(p)
                .map_err(|e| JsonRpcError { code: -32602, message: e })?;
            let wait_secs = p
                .get("wait_ready_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(30);

            let svc_ref = &**svc;
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
            let result = svc_ref
                .spawn_agent_sync_with_ready(spawn_params, Some(ready_tx))
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            { let h = app.clone(); std::thread::spawn(move || { let _ = h.emit("mux-state-changed", ()); }); }

            // Socket dispatch runs on a plain std::thread; poll the oneshot
            // synchronously rather than spinning up a tokio runtime, which is
            // brittle here and dispatch-handler panics close the connection.
            let mut ready_rx = ready_rx;
            let deadline = std::time::Instant::now() + Duration::from_secs(wait_secs);
            let (ready, ready_error) = loop {
                match ready_rx.try_recv() {
                    Ok(Ok(())) => break (true, None),
                    Ok(Err(reason)) => break (false, Some(reason)),
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed) => break (
                        false,
                        Some("internal: launch thread dropped before signaling".to_string()),
                    ),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                        if std::time::Instant::now() >= deadline {
                            break (
                                false,
                                Some(format!("timed out ({}s) waiting for agent readiness", wait_secs)),
                            );
                        }
                        std::thread::sleep(Duration::from_millis(40));
                    }
                }
            };

            let mut resp = serde_json::json!({
                "pane_id": result.pane_id,
                "tab_id": result.tab_id,
                "agent": result.agent,
                "title": result.title,
                "role": result.role,
                "ready": ready,
            });
            if let Some(reason) = ready_error {
                resp["ready_error"] = serde_json::json!(reason);
            }
            if let Some(ref warning) = result.warning {
                resp["warning"] = serde_json::json!(warning);
            }
            Ok(resp)
        }

        "agent.report" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let message = get_string_param(params, "message")?;
            let svc_ref = &**svc;
            let result = svc_ref.submit_report(&pane_id, &message)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({
                "ok": true,
                "report_id": result.report_id,
            }))
        }

        "agent.reports" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let clear = params.as_ref()
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

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}
