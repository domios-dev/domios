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
                .unwrap_or(false);
            let agents_only = params
                .as_ref()
                .and_then(|value| value.get("agents_only"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            crate::dispatch::dispatch_tab_summary_if_ready(
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
            let detach_from_team = params
                .as_ref()
                .and_then(|p| p.get("detach_from_team"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
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
                    detach_from_team,
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
            let writes_pty = svc.as_ref().tab_chat_backing(&resolved_target).is_none();
            if writes_pty {
                if svc.as_ref().has_public_send_launch_gate(&resolved_target) {
                    if let Err(detail) = svc.as_ref().await_public_send_readiness(
                        &resolved_target,
                        std::time::Duration::from_secs(30),
                    ) {
                        return Ok(local_send_response(
                            Err(
                                crate::service::delivery_error::DeliveryError::TabBusyRefused {
                                    tab: resolved_target,
                                    detail,
                                },
                            ),
                            true,
                        ));
                    }
                }
            }
            let response = if let Some(src) = source_tab.as_deref() {
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
                let delivery = svc
                    .as_ref()
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
                gateway_send_response(delivery)
            } else {
                local_send_response(
                    svc.as_ref()
                        .send_text_input_delivery(&resolved_target, &raw_text, enter, None),
                    writes_pty,
                )
            };
            Ok(response)
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

fn gateway_send_response(delivery: crate::service::messenger::Delivery) -> Value {
    match delivery {
        crate::service::messenger::Delivery::Pty { .. } => serde_json::json!({
            "ok": true,
            "delivery": "pending",
            "bytes_written": false,
        }),
        crate::service::messenger::Delivery::ChatBackend { .. } => serde_json::json!({
            "ok": true,
            "delivery": "sent",
        }),
    }
}

fn local_send_response(
    result: Result<(), crate::service::delivery_error::DeliveryError>,
    writes_pty: bool,
) -> Value {
    match result {
        Ok(()) if writes_pty => serde_json::json!({
            "ok": true,
            "delivery": "sent",
            "bytes_written": true,
        }),
        Ok(()) => serde_json::json!({ "ok": true, "delivery": "sent" }),
        Err(error) if error.bytes_written() => serde_json::json!({
            "ok": true,
            "delivery": "pending",
            "bytes_written": true,
        }),
        Err(error) => error.to_json(),
    }
}

#[cfg(test)]
mod tests {
    use super::{gateway_send_response, local_send_response, require_send_target};
    use crate::service::delivery_error::DeliveryError;
    use crate::service::messenger::Delivery;

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

    #[test]
    fn local_send_response_reports_confirmed_pending_and_refused_outcomes() {
        assert_eq!(
            local_send_response(Ok(()), true),
            serde_json::json!({
                "ok": true,
                "delivery": "sent",
                "bytes_written": true,
            }),
        );
        assert_eq!(
            local_send_response(
                Err(DeliveryError::SubmitUnconfirmed {
                    tab: "tab".into(),
                    detail: "submit was not confirmed".into(),
                }),
                true,
            ),
            serde_json::json!({
                "ok": true,
                "delivery": "pending",
                "bytes_written": true,
            }),
        );
        let refused = local_send_response(
            Err(DeliveryError::TabBusyRefused {
                tab: "tab".into(),
                detail: "provider is not ready".into(),
            }),
            true,
        );
        assert_eq!(refused["ok"], false);
        assert_eq!(refused["error"]["code"], "tab_busy_refused");
        assert_eq!(refused["error"]["bytes_written"], false);
    }

    #[test]
    fn chat_backend_send_does_not_claim_pty_bytes() {
        assert_eq!(
            local_send_response(Ok(()), false),
            serde_json::json!({ "ok": true, "delivery": "sent" }),
        );
    }

    #[test]
    fn enveloped_pty_send_remains_pending_until_background_write() {
        assert_eq!(
            gateway_send_response(Delivery::Pty {
                text: "body".into()
            }),
            serde_json::json!({
                "ok": true,
                "delivery": "pending",
                "bytes_written": false,
            }),
        );
    }
}
