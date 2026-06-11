//! `workspace.*` JSON-RPC handlers.

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
        "workspace.list" => {
            crate::dispatch::dispatch(&crate::dispatch::Command::WorkspaceList, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })
        }

        "workspace.create" => {
            let name = params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("New Workspace")
                .to_string();
            let cwd = params
                .as_ref()
                .and_then(|p| p.get("cwd"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let activate = params
                .as_ref()
                .and_then(|p| p.get("activate"))
                .and_then(|v| v.as_bool());

            crate::dispatch::dispatch(
                &crate::dispatch::Command::WorkspaceCreate {
                    name,
                    cwd,
                    activate,
                },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
            })
        }

        "workspace.create_tab" => {
            let workspace_id = get_string_param(params, "workspace_id")?;
            let eager = params
                .as_ref()
                .and_then(|p| p.get("eager"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let (tab_id, pane_id) = svc
                .create_tab_with_activate(&workspace_id, false)
                .map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?;
            let _ = svc.set_pane_role(&pane_id, "Orchestrator", None, Some(true));
            let pty_id = if eager {
                svc.ensure_pane_pty_for_send(&pane_id)
                    .map_err(|e| JsonRpcError {
                        code: -32603,
                        message: e,
                    })?;
                svc.mux.lock().get_pane(&pane_id).and_then(|p| p.pty_id.clone())
            } else {
                None
            };
            Ok(serde_json::json!({
                "tab_id": tab_id,
                "pane_id": pane_id,
                "pty_id": pty_id
            }))
        }

        "workspace.focus" => {
            let id = get_string_param(params, "workspace_id")?;
            {
                let mut mux = mux.lock();
                mux.set_active_workspace(&id).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?;
            }
            {
                let h = app.clone();
                std::thread::spawn(move || {
                    let _ = h.emit("mux-state-changed", ());
                });
            }
            Ok(serde_json::json!({"ok": true}))
        }

        "workspace.rename" => {
            let workspace_id = get_string_param(params, "workspace_id")?;
            let name = get_string_param(params, "name")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::WorkspaceRename { workspace_id, name },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
            })
        }

        "workspace.close" => {
            let workspace_id = get_string_param(params, "workspace_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::WorkspaceClose { workspace_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
            })
        }

        "workspace.move_tab" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let from_id = get_string_param(params, "from_workspace_id")?;
            let to_id = get_string_param(params, "to_workspace_id")?;
            {
                let mut mux = mux.lock();
                mux.move_tab(&tab_id, &from_id, &to_id)
                    .map_err(|e| JsonRpcError {
                        code: -32602,
                        message: e,
                    })?;
            }
            {
                let h = app.clone();
                std::thread::spawn(move || {
                    let _ = h.emit("mux-state-changed", ());
                });
            }
            Ok(serde_json::json!({"ok": true}))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}
