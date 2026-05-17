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
            let raw_text = get_string_param(params, "text")?;
            let enter = params
                .as_ref()
                .and_then(|p| p.get("enter"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let source_pane = params
                .as_ref()
                .and_then(|p| p.get("source_pane"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty() && s != &pane_id);
            // Source-prefix only for agent recipients; shells just execute the bytes.
            let target_has_agent = mux.lock().get_pane(&pane_id)
                .map(|p| p.agent_type.is_some())
                .unwrap_or(false);
            let text = match source_pane.as_deref() {
                Some(src) if target_has_agent => {
                    let label = mux.lock().display_label_for_pane(src);
                    let short = src.chars().take(8).collect::<String>();
                    format!("[from {} pane={}] {}", label, short, raw_text)
                }
                _ => raw_text,
            };
            svc.as_ref().send_text(&pane_id, &text, enter)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({"ok": true}))
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

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}
