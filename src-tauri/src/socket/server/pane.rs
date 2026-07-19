//! `pane.*`, `agents.list_providers`, and `browser.open` JSON-RPC handlers.

use serde_json::Value;
use tauri::AppHandle;

use crate::multiplexer::commands::MuxState;
use crate::service::ServiceState;

use super::{get_string_param, opt_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    mux: &MuxState,
    _app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "pane.list" => crate::dispatch::dispatch(&crate::dispatch::Command::PaneList, svc.as_ref())
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
                data: None,
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
                    data: None,
                })?;
            let mut resp = serde_json::json!({
                "id": handle.pane_id,
                "pane_id": handle.pane_id,
                "workspace_id": handle.workspace_id,
                "tab_id": handle.tab_id,
                "pty_id": handle.pty_id
            });
            if let Some(ref notice) = handle.budget_notice {
                resp["budget_notice"] = serde_json::to_value(notice).unwrap_or_default();
            }
            Ok(resp)
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
                data: None,
            })
        }

        "pane.close" => {
            let pane_id = get_string_param(params, "pane_id")?;
            let caller_pane_id = opt_string_param(params, "caller_pane_id");
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneClose {
                    pane_id,
                    caller_pane_id,
                },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
                data: None,
            })
        }

        "agents.list_providers" => {
            crate::dispatch::dispatch(&crate::dispatch::Command::ListAgentProviders, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
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
            // Resolve target via alias-aware lookup so short ids (`--tab ce9bce56`) and labels work.
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
                svc.as_ref()
                    .gateway_deliver(
                        &resolved_target,
                        envelope,
                        &raw_text,
                        crate::service::message_gateway::DeliveryClass::AgentTraffic,
                    )
                    .map_err(|e| JsonRpcError {
                        code: -32603,
                        message: e,
                        data: None,
                    })?;
                if !raw_text.trim().is_empty() {
                    svc.as_ref()
                        .maybe_send_owned_plan_reminder(&resolved_target);
                }
            } else {
                svc.as_ref()
                    .send_text_detached(&resolved_target, &raw_text, enter);
                if !raw_text.trim().is_empty() {
                    svc.as_ref()
                        .maybe_send_owned_plan_reminder(&resolved_target);
                }
            }
            Ok(serde_json::json!({"ok": true}))
        }

        "pane.get_output" => {
            let pane_id = get_string_param(params, "pane_id")?;
            if let Some(src) = params
                .as_ref()
                .and_then(|p| p.get("source_pane"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                let (caller, target) = {
                    let m = mux.write();
                    (
                        m.resolve_pane_id(src).unwrap_or_else(|| src.to_string()),
                        m.resolve_pane_id(&pane_id)
                            .unwrap_or_else(|| pane_id.clone()),
                    )
                };
                svc.as_ref().maybe_send_polling_nudge(&caller, &target);
            }
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
                data: None,
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
                data: None,
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
                data: None,
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
                    data: None,
                })? as u16;
            let rows = params
                .as_ref()
                .and_then(|p| p.get("rows"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| JsonRpcError {
                    code: -32602,
                    message: "missing rows".into(),
                    data: None,
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
                data: None,
            })
        }

        "pane.kill_pty" => {
            let pane_id = get_string_param(params, "pane_id")?;
            crate::dispatch::dispatch(&crate::dispatch::Command::PtyKill { pane_id }, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
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
