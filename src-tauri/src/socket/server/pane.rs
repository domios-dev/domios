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
        "pane.list" => crate::dispatch::dispatch(&crate::dispatch::Command::PaneList, svc.as_ref())
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
            }),

        "pane.create" | "pane.new" => {
            let workspace_id = params
                .as_ref()
                .and_then(|p| p.get("workspace_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let cwd = params
                .as_ref()
                .and_then(|p| p.get("cwd"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let title = params
                .as_ref()
                .and_then(|p| p.get("title"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let eager = params
                .as_ref()
                .and_then(|p| p.get("eager"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let handle = svc
                .spawn_pane(crate::service::spawn::SpawnPaneSpec {
                    workspace_id,
                    cwd,
                    title,
                    visibility: crate::service::spawn::PaneVisibility::Visible,
                    eager,
                    command: crate::service::spawn::PaneCommand::Shell,
                    ..Default::default()
                })
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })?;
            Ok(serde_json::json!({
                "id": handle.pane_id,
                "pane_id": handle.pane_id,
                "workspace_id": handle.workspace_id,
                "tab_id": handle.tab_id,
                "pty_id": handle.pty_id
            }))
        }

        "pane.focus" => {
            let pane_id = get_string_param(params, "pane_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneFocus { pane_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
            })
        }

        "pane.close" => {
            let pane_id = get_string_param(params, "pane_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneClose { pane_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
            })
        }

        "agents.list_providers" => {
            crate::dispatch::dispatch(&crate::dispatch::Command::ListAgentProviders, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })
        }

        "pane.send_text" => {
            let raw_pane_id = get_string_param(params, "pane_id")?;
            let raw_text = get_string_param(params, "text")?;
            let enter = params
                .as_ref()
                .and_then(|p| p.get("enter"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            // Resolve target via the alias-aware lookup so short ids (`--pane ce9bce56`) and tab labels both work.
            let resolved_target = {
                let m = mux.write();
                m.resolve_pane_id(&raw_pane_id)
                    .unwrap_or_else(|| raw_pane_id.clone())
            };
            let source_pane = params
                .as_ref()
                .and_then(|p| p.get("source_pane"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .and_then(|s| mux.write().resolve_pane_id(&s));
            if let Some(src) = source_pane.as_deref() {
                let (from_label, model) = {
                    let m = mux.write();
                    (
                        m.display_label_for_pane(src),
                        m.get_pane(src).and_then(|p| p.agent_session_model.clone()),
                    )
                };
                let envelope =
                    crate::service::messenger::MessageEnvelope::pane(from_label, src.to_string())
                        .model(model);
                crate::service::messenger::Messenger::deliver(
                    svc.as_ref(),
                    &resolved_target,
                    envelope,
                    &raw_text,
                )
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })?;
            } else {
                svc.as_ref()
                    .send_text(&resolved_target, &raw_text, enter)
                    .map_err(|e| JsonRpcError {
                        code: -32603,
                        message: e,
                    })?;
            }
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
            let strip_ansi = params
                .as_ref()
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
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
            })
        }

        "pane.rename" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let title = get_string_param(params, "title")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneRename { pane_id, title },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
            })
        }

        "pane.describe" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let description = get_string_param(params, "description")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneDescribe {
                    pane_id,
                    description,
                },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
            })
        }

        "pane.resize" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let cols = params
                .as_ref()
                .and_then(|p| p.get("cols"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| JsonRpcError {
                    code: -32602,
                    message: "missing cols".into(),
                })? as u16;
            let rows = params
                .as_ref()
                .and_then(|p| p.get("rows"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| JsonRpcError {
                    code: -32602,
                    message: "missing rows".into(),
                })? as u16;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PtyResize {
                    pane_id,
                    cols,
                    rows,
                },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
            })
        }

        "pane.kill_pty" => {
            let pane_id = get_string_param(params, "pane_id")?;
            crate::dispatch::dispatch(&crate::dispatch::Command::PtyKill { pane_id }, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}
