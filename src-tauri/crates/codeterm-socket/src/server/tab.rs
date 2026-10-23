//! `tab.*` and `agents.list_providers` JSON-RPC handlers.

use serde_json::Value;

use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::service::ServiceState;

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
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    if method.starts_with("tab.todo.") {
        return super::tab_todos::dispatch(method, params, svc);
    }
    match method {
        "tab.shells" => {
            let refresh = params
                .as_ref()
                .and_then(|p| p.get("refresh"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::ShellProfiles { refresh },
                svc.as_ref(),
            )
            .map(|result| result.into_json())
            .map_err(|message| JsonRpcError {
                code: -32603,
                message,
                data: None,
            })
        }
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
            codeterm_service_core::dispatch::dispatch_tab_summary_if_ready(
                &codeterm_service_core::dispatch::Command::TabSummary {
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
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::TabInspect { tab_ids, lifecycle },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| {
                let missing = e.contains("not found");
                JsonRpcError {
                    code: if missing { -32602 } else { -32603 },
                    message: e,
                    data: missing.then(|| serde_json::json!({ "code": "not_found" })),
                }
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
            let purpose = params
                .as_ref()
                .and_then(|p| p.get("purpose"))
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| JsonRpcError {
                    code: -32602,
                    message: format!("invalid shell purpose: {error}"),
                    data: None,
                })?
                .unwrap_or_default();
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
                .spawn_tab(codeterm_service_core::service::spawn::SpawnTabSpec {
                    shell: opt_string_param(params, "shell"),
                    cmd: opt_string_param(params, "cmd"),
                    purpose,
                    workspace_id,
                    team_id,
                    creator_tab_id,
                    detach_from_team,
                    cwd,
                    title,
                    visibility: codeterm_service_core::service::spawn::TabVisibility::Visible,
                    eager,
                    command: codeterm_service_core::service::spawn::TabCommand::Shell,
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
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::TabFocus { tab_id },
                svc.as_ref(),
            )
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
            let force = params
                .as_ref()
                .and_then(|params| params.get("force"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::TabClose {
                    tab_id,
                    caller_tab_id,
                    force,
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

        "agents.cli_run" => {
            let provider = get_string_param(params, "provider")?;
            let raw = get_string_param(params, "operation")?;
            let operation = serde_json::from_value::<
                codeterm_service_core::service::provider_cli_actions::CliOperation,
            >(Value::String(raw))
            .map_err(|error| JsonRpcError {
                code: -32602,
                message: error.to_string(),
                data: None,
            })?;
            let caller = opt_string_param(params, "caller_tab_id");
            codeterm_service_core::service::provider_cli_actions::launch(
                svc, &provider, operation, caller,
            )
            .and_then(|result| serde_json::to_value(result).map_err(|e| e.to_string()))
            .map_err(|message| JsonRpcError {
                code: -32602,
                message,
                data: None,
            })
        }

        "agents.cli_status" => {
            let provider = get_string_param(params, "provider")?;
            codeterm_service_core::service::provider_cli::provider_cli_status(
                &svc.provider_registry,
                &provider,
                codeterm_service_core::agent::provider::current_platform_tag(),
            )
            .and_then(|status| serde_json::to_value(status).map_err(|e| e.to_string()))
            .map_err(|message| JsonRpcError {
                code: -32602,
                message,
                data: None,
            })
        }

        "agents.list_providers" => codeterm_service_core::dispatch::dispatch(
            &codeterm_service_core::dispatch::Command::ListAgentProviders,
            svc.as_ref(),
        )
        .map(|r| r.into_json())
        .map_err(|e| JsonRpcError {
            code: -32603,
            message: e,
            data: None,
        }),

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
                let m = mux.read();
                require_send_target(&raw_tab_id, m.resolve_tab_id(&raw_tab_id))?
            };
            let source_tab = params
                .as_ref()
                .and_then(|p| p.get("source_tab"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .and_then(|s| mux.write().resolve_tab_id(&s));
            if let Err(locked) = svc.as_ref().tab_write_lock_delivery(&resolved_target) {
                return Ok(local_send_response(Err(locked), true, None));
            }
            svc.as_ref()
                .require_managed_send_allowed(&resolved_target, source_tab.as_deref())
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            let writes_pty = svc.as_ref().tab_chat_backing(&resolved_target).is_none();
            let target_has_agent = mux
                .read()
                .get_tab(&resolved_target)
                .is_some_and(|tab| tab.agent_type.is_some() || tab.agent_booting);
            if writes_pty && svc.as_ref().has_public_send_launch_gate(&resolved_target) {
                if let Err(detail) = svc.as_ref().await_public_send_readiness(
                    &resolved_target,
                    std::time::Duration::from_secs(30),
                ) {
                    return Ok(local_send_response(
                        Err(
                            codeterm_service_core::service::delivery_error::DeliveryError::TabBusyRefused {
                                tab: resolved_target,
                                detail,
                            },
                        ),
                        true,
                        None,
                    ));
                }
            }
            let system_context = opt_string_param(params, "system_context");
            let response = if source_tab.is_some() || target_has_agent || system_context.is_some() {
                let src = source_tab.as_deref().unwrap_or("");
                let (from_label, model, from_agent) = {
                    let m = mux.read();
                    let source = m.get_tab(src);
                    (
                        m.display_label_for_tab(src),
                        source.and_then(|tab| tab.agent_session_model.clone()),
                        source.is_some_and(|tab| tab.agent_type.is_some() || tab.agent_booting),
                    )
                };
                let envelope = codeterm_service_core::service::messenger::MessageEnvelope::tab(
                    from_label,
                    src.to_string(),
                )
                .model(model)
                .from_agent(Some(from_agent));
                let envelope = signed_send_envelope(envelope, system_context);
                let class = if envelope.kind
                    == codeterm_service_core::service::messenger::MessageKind::System
                {
                    codeterm_service_core::service::message_gateway::DeliveryClass::SystemNotice
                } else {
                    codeterm_service_core::service::message_gateway::DeliveryClass::AgentTraffic
                };
                let (delivery, pad_file) = svc
                    .as_ref()
                    .gateway_deliver_with_pad_file(&resolved_target, envelope, &raw_text, class)
                    .map_err(|e| JsonRpcError {
                        code: -32603,
                        message: e,
                        data: None,
                    })?;
                gateway_send_response(delivery, pad_file)
            } else {
                let (text, pad_file) = svc
                    .as_ref()
                    .substitute_paste_limited_body(
                        &resolved_target,
                        raw_text.clone(),
                        raw_text.len(),
                    )
                    .map_err(|message| JsonRpcError {
                        code: -32603,
                        message,
                        data: None,
                    })?;
                local_send_response(
                    svc.as_ref().send_text_input_delivery(
                        &resolved_target,
                        &codeterm_service_core::service::messenger::AgentInput::Human(
                            codeterm_service_core::service::messenger::HumanInput::composer(&text),
                        ),
                        enter,
                        None,
                    ),
                    writes_pty,
                    pad_file,
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
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::GetOutput {
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
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::TabRename { tab_id, title },
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
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::TabDescribe {
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
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::PtyResize { tab_id, cols, rows },
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
            codeterm_service_core::dispatch::dispatch(
                &codeterm_service_core::dispatch::Command::PtyKill { tab_id },
                svc.as_ref(),
            )
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

fn signed_send_envelope(
    source: codeterm_service_core::service::messenger::MessageEnvelope,
    system_context: Option<String>,
) -> codeterm_service_core::service::messenger::MessageEnvelope {
    system_context.map_or(
        source,
        codeterm_service_core::service::messenger::MessageEnvelope::codeterm_reminder,
    )
}

fn gateway_send_response(
    delivery: codeterm_service_core::service::messenger::Delivery,
    pad_file: Option<std::path::PathBuf>,
) -> Value {
    let mut value = match delivery {
        codeterm_service_core::service::messenger::Delivery::Pty { .. } => serde_json::json!({
            "ok": true,
            "delivery": "sent",
            "bytes_written": true,
        }),
        codeterm_service_core::service::messenger::Delivery::ChatBackend { message_id } => {
            serde_json::json!({
                "ok": true,
                "delivery": "sent",
                "bytes_written": true,
                "send_id": message_id,
            })
        }
        codeterm_service_core::service::messenger::Delivery::Unconfirmed { delivery_id } => {
            serde_json::json!({
                "ok": true,
                "delivery": "pending",
                "bytes_written": true,
                "send_id": delivery_id,
            })
        }
        codeterm_service_core::service::messenger::Delivery::Pending { .. } => serde_json::json!({
            "ok": true,
            "delivery": "pending",
            "bytes_written": false,
        }),
    };
    if let Some(path) = pad_file {
        value["pad_file"] = serde_json::json!(path.display().to_string());
    }
    value
}

fn local_send_response(
    result: Result<(), codeterm_service_core::service::delivery_error::DeliveryError>,
    writes_pty: bool,
    pad_file: Option<std::path::PathBuf>,
) -> Value {
    let mut value = match result {
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
    };
    if let Some(path) = pad_file {
        value["pad_file"] = serde_json::json!(path.display().to_string());
    }
    value
}

#[cfg(test)]
mod tests;
