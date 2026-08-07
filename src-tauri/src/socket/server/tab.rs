//! `tab.*` and `agents.list_providers` JSON-RPC handlers.

use serde_json::Value;
use tauri::AppHandle;

use crate::multiplexer::commands::MuxState;
use crate::service::ServiceState;

use super::{get_string_param, opt_string_param, JsonRpcError};

fn require_send_target(raw_tab_id: &str, resolved: Option<String>) -> Result<String, JsonRpcError> {
    resolved.ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!("Tab not found: {raw_tab_id}"),
        data: None,
    })
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    mux: &MuxState,
    _app: &AppHandle,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "tab.list" => {
            let workspace_id = opt_string_param(params, "workspace_id");
            let caller_tab_id = opt_string_param(params, "caller_tab_id");
            let focused_workspace = params
                .as_ref()
                .and_then(|value| value.get("focused_workspace"))
                .and_then(|value| value.as_bool())
                .unwrap_or(workspace_id.is_none());
            let agents_only = params
                .as_ref()
                .and_then(|value| value.get("agents_only"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            crate::dispatch::dispatch(
                &crate::dispatch::Command::TabSummary {
                    workspace_id,
                    caller_tab_id,
                    focused_workspace,
                    agents_only,
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
        "tab.inspect" => {
            let tab_ids = params
                .as_ref()
                .and_then(|value| value.get("tab_ids"))
                .and_then(|value| value.as_array())
                .ok_or_else(|| JsonRpcError {
                    code: -32602,
                    message: "tab.inspect requires tab_ids".into(),
                    data: None,
                })?
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| JsonRpcError {
                    code: -32602,
                    message: "tab_ids must contain strings".into(),
                    data: None,
                })?;
            let lifecycle = params
                .as_ref()
                .and_then(|value| value.get("lifecycle"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            crate::dispatch::dispatch(
                &crate::dispatch::Command::TabInspect { tab_ids, lifecycle },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
                data: None,
            })
        }

        "tab.agent_pad" => {
            let tab_id = get_string_param(params, "tab_id")?;
            if mux.read().get_tab(&tab_id).is_none() {
                return Err(JsonRpcError {
                    code: -32602,
                    message: format!("Tab {tab_id} not found"),
                    data: None,
                });
            }
            let pad = svc
                .ensure_agent_pad(&tab_id)
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            Ok(serde_json::json!({
                "tab_id": tab_id,
                "path": pad.path(),
            }))
        }

        "tab.create" | "tab.new" => {
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
            let team_id = opt_string_param(params, "team_id");
            let creator_tab_id = opt_string_param(params, "creator_tab_id");
            let eager = params
                .as_ref()
                .and_then(|p| p.get("eager"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let handle = svc
                .spawn_tab(crate::service::spawn::SpawnTabSpec {
                    workspace_id,
                    team_id,
                    creator_tab_id,
                    cwd,
                    title,
                    visibility: crate::service::spawn::TabVisibility::Visible,
                    eager,
                    command: crate::service::spawn::TabCommand::Shell,
                    ..Default::default()
                })
                .map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                    data: None,
                })?;
            let mut resp = serde_json::json!({
                "id": handle.tab_id,
                "tab_id": handle.tab_id,
                "workspace_id": handle.workspace_id,
                "tab_id": handle.tab_id,
                "pty_id": handle.pty_id
            });
            if let Some(ref notice) = handle.budget_notice {
                resp["budget_notice"] = serde_json::to_value(notice).unwrap_or_default();
            }
            Ok(resp)
        }

        "tab.focus" => {
            let tab_id = get_string_param(params, "tab_id")?;
            crate::dispatch::dispatch(&crate::dispatch::Command::TabFocus { tab_id }, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                    data: None,
                })
        }

        "tab.close" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let caller_tab_id = opt_string_param(params, "caller_tab_id");
            crate::dispatch::dispatch(
                &crate::dispatch::Command::TabClose {
                    tab_id,
                    caller_tab_id,
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

        "tab.send_text" => {
            let raw_tab_id = get_string_param(params, "tab_id")?;
            let raw_text = get_string_param(params, "text")?;
            let enter = params
                .as_ref()
                .and_then(|p| p.get("enter"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            // Resolve target via alias-aware lookup so short ids (`--tab ce9bce56`) and labels work.
            let resolved_target = {
                let m = mux.write();
                require_send_target(&raw_tab_id, m.resolve_tab_id(&raw_tab_id))?
            };
            let source_tab = params
                .as_ref()
                .and_then(|p| p.get("source_tab"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .and_then(|s| mux.write().resolve_tab_id(&s));
            if let Some(src) = source_tab.as_deref() {
                let (from_label, model) = {
                    let m = mux.write();
                    (
                        m.display_label_for_tab(src),
                        m.get_tab(src).and_then(|p| p.agent_session_model.clone()),
                    )
                };
                let envelope =
                    crate::service::messenger::MessageEnvelope::tab(from_label, src.to_string())
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
            } else {
                if enter {
                    svc.send_text_detached(&resolved_target, &raw_text);
                } else {
                    svc.insert_text_detached(&resolved_target, &raw_text);
                }
            }
            Ok(serde_json::json!({"ok": true}))
        }

        "tab.get_output" => {
            let tab_id = get_string_param(params, "tab_id")?;
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
                    tab_id,
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

        "tab.rename" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let title = get_string_param(params, "title")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::TabRename { tab_id, title },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
                data: None,
            })
        }

        "tab.describe" => {
            let tab_id = get_string_param(params, "tab_id")?;
            let description = get_string_param(params, "description")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::TabDescribe {
                    tab_id,
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

        "tab.resize" => {
            let tab_id = get_string_param(params, "tab_id")?;
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
                &crate::dispatch::Command::PtyResize { tab_id, cols, rows },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
                data: None,
            })
        }

        "tab.kill_pty" => {
            let tab_id = get_string_param(params, "tab_id")?;
            crate::dispatch::dispatch(&crate::dispatch::Command::PtyKill { tab_id }, svc.as_ref())
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

#[cfg(test)]
mod tests {
    use super::require_send_target;

    #[test]
    fn send_target_must_resolve_to_a_real_tab() {
        let error = require_send_target("system", None).unwrap_err();
        assert_eq!(error.code, -32602);
        assert_eq!(error.message, "Tab not found: system");
        assert_eq!(
            require_send_target("short", Some("full-id".into())).unwrap(),
            "full-id"
        );
    }
}
