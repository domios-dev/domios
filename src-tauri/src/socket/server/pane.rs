//! `pane.*`, `agents.list_providers`, and `browser.open` JSON-RPC handlers.

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::multiplexer::commands::MuxState;
use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    mux: &MuxState,
    app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "pane.list" => {
            crate::dispatch::dispatch(&crate::dispatch::Command::PaneList, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "pane.focus" => {
            let pane_id = get_string_param(params, "pane_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneFocus { pane_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "pane.close" => {
            let pane_id = get_string_param(params, "pane_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneClose { pane_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "agents.list_providers" => {
            crate::dispatch::dispatch(
                &crate::dispatch::Command::ListAgentProviders,
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "pane.send_text" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let text = get_string_param(params, "text")?;
            let enter = params
                .as_ref()
                .and_then(|p| p.get("enter"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let verify = params
                .as_ref()
                .and_then(|p| p.get("verify"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let svc_ref = svc.as_ref();
            if verify {
                let pty_id = svc_ref.resolve_pty_id(&pane_id)
                    .map_err(|e| JsonRpcError { code: -32602, message: e })?;
                let opts = crate::service::inject_verify::VerifyOpts::default();
                let strategy = crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
                    timeout: std::time::Duration::from_secs(10),
                };
                return Ok(match svc_ref.send_text_verified(&pty_id, &text, enter, strategy, opts) {
                    Ok(r) => serde_json::json!({
                        "ok": true,
                        // `verified` reflects best-available evidence: both echo
                        // (text rendered on screen) AND submit (Enter consumed
                        // by the TUI) were observed. Falsy here does NOT mean
                        // delivery failed — Enter is always sent regardless;
                        // it just means we couldn't confirm via screen state.
                        "verified": r.echo_observed && (!enter || r.submit_observed),
                        "echo_observed": r.echo_observed,
                        "submit_observed": r.submit_observed,
                        "elapsed_ms": r.elapsed.as_millis() as u64,
                    }),
                    Err(e) => serde_json::json!({
                        "ok": false,
                        "verified": false,
                        "error": format!("{}", e),
                    }),
                });
            }
            crate::dispatch::dispatch(
                &crate::dispatch::Command::SendText { pane_id, text, enter },
                svc_ref,
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "pane.get_output" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let max_bytes = params
                .as_ref()
                .and_then(|p| p.get("max_bytes"))
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .or(Some(65536));
            let strip_ansi = params.as_ref()
                .and_then(|p| p.get("strip_ansi"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            crate::dispatch::dispatch(
                &crate::dispatch::Command::GetOutput {
                    pane_id,
                    offset: None,
                    max_bytes,
                    strip_ansi,
                    full: false,
                },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "pane.rename" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let title = get_string_param(params, "title")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneRename { pane_id, title },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "pane.resize" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let cols = params
                .as_ref()
                .and_then(|p| p.get("cols"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| JsonRpcError { code: -32602, message: "missing cols".into() })?
                as u16;
            let rows = params
                .as_ref()
                .and_then(|p| p.get("rows"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| JsonRpcError { code: -32602, message: "missing rows".into() })?
                as u16;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PtyResize { pane_id, cols, rows },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "pane.kill_pty" => {
            let pane_id = get_string_param(params, "pane_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PtyKill { pane_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "browser.open" => {
            let url = get_string_param(params, "url")?;
            let new_pane_id = {
                let mut mux = mux.lock();
                mux.open_browser(Some(url.clone())).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })?
            };
            { let h = app.clone(); std::thread::spawn(move || { let _ = h.emit("mux-state-changed", ()); }); }
            Ok(serde_json::json!({"pane_id": new_pane_id, "url": url}))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}
