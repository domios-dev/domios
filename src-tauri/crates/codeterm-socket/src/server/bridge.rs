//! `bridge.*` JSON-RPC handlers (Telegram outbound + reactions + restart).

use serde_json::Value;

use codeterm_service_core::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "bridge.restart" => {
            codeterm_service_core::spawn_telegram_bridge(svc.clone());
            Ok(serde_json::json!({"ok": true, "message": "Telegram bridge starting"}))
        }

        "bridge.send" => {
            let service = params
                .as_ref()
                .and_then(|p| p.get("service"))
                .and_then(|v| v.as_str())
                .unwrap_or("telegram");
            codeterm_service_core::bridge::manager::ensure_supported_service(service).map_err(
                |msg| JsonRpcError {
                    code: -32602,
                    message: msg.to_string(),
                    data: None,
                },
            )?;

            let chat_id = params
                .as_ref()
                .and_then(|p| p.get("chat_id"))
                .and_then(|v| v.as_i64());
            let append = params
                .as_ref()
                .and_then(|p| p.get("append"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let content = codeterm_service_core::bridge::manager::outbound_content(params)
                .map_err(|message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                })?;
            let is_media = !matches!(
                content,
                codeterm_service_core::bridge::traits::OutboundContent::Text { .. }
            );

            let buttons: Option<codeterm_service_core::bridge::traits::InlineKeyboard> = params
                .as_ref()
                .and_then(|p| p.get("buttons"))
                .and_then(|v| match serde_json::from_value(v.clone()) {
                    Ok(b) => Some(b),
                    Err(e) => {
                        log::warn!("[bridge.send] failed to parse buttons: {} — raw: {}", e, v);
                        None
                    }
                });

            // A send from a live tab speaks for that tab; anything else stays an unattributed chat send.
            let route = params
                .as_ref()
                .and_then(|p| p.get("caller_tab_id"))
                .and_then(|v| v.as_str())
                .and_then(|caller| svc.mux.read().resolve_tab_id(caller.trim()))
                .map(|tab_id| codeterm_service_core::bridge::manager::OutboundRoute::Tab { tab_id })
                .unwrap_or_default();

            let tx = svc.bridge_outbound.lock().clone();
            if let Some(tx) = tx {
                let (completion, delivered) = std::sync::mpsc::sync_channel(1);
                let msg = codeterm_service_core::bridge::manager::OutboundMessage {
                    user_id: chat_id,
                    content,
                    append,
                    buttons,
                    edit: None,
                    completion: is_media.then_some(completion),
                    receipt: None,
                    route,
                };
                tx.send(msg).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: format!("Bridge send failed: {}", e),
                    data: None,
                })?;
                if is_media {
                    delivered.recv_timeout(std::time::Duration::from_secs(70))
                        .map_err(|error| JsonRpcError {
                            code: -32603,
                            message: format!("Telegram upload delivery unconfirmed: {error}; do not retry automatically"),
                            data: None,
                        })?
                        .map_err(|cause| JsonRpcError {
                            code: -32603,
                            message: format!("Telegram upload failed: {cause}"),
                            data: None,
                        })?;
                }
                Ok(serde_json::json!({"ok": true}))
            } else {
                Err(JsonRpcError {
                    code: -32603,
                    message: "Bridge not active".to_string(),
                    data: None,
                })
            }
        }

        "bridge.thread.open" => open_thread(params, svc),

        "bridge.react" => {
            let chat_id = params
                .as_ref()
                .and_then(|p| p.get("chat_id"))
                .and_then(|v| v.as_i64())
                .ok_or(JsonRpcError {
                    code: -32602,
                    message: "chat_id required".into(),
                    data: None,
                })?;
            let message_id = params
                .as_ref()
                .and_then(|p| p.get("message_id"))
                .and_then(|v| v.as_i64())
                .ok_or(JsonRpcError {
                    code: -32602,
                    message: "message_id required".into(),
                    data: None,
                })?;
            let emoji = get_string_param(params, "emoji")?;

            let token = codeterm_service_core::bridge::telegram::read_token(
                &codeterm_service_core::bridge_host::AppAttachmentStorePort,
            )
            .unwrap_or_default();
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                let resp = client
                    .post(format!(
                        "https://api.telegram.org/bot{}/setMessageReaction",
                        token
                    ))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": message_id,
                        "reaction": [{"type": "emoji", "emoji": emoji}],
                    }))
                    .timeout(std::time::Duration::from_secs(10))
                    .send()
                    .await;
                match resp {
                    Ok(r) => {
                        let body: serde_json::Value = r.json().await.unwrap_or_default();
                        if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
                            log::warn!(
                                "[bridge] react failed: {}",
                                body.get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                            );
                        }
                    }
                    Err(e) => log::warn!("[bridge] react error: {}", e),
                }
            });
            Ok(serde_json::json!({"ok": true}))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

/// Same refusal class as a stale task binding: the caller lacks authority.
const THREAD_AUTHORITY_CODE: i32 = -32003;

fn thread_param(params: &Option<Value>, key: &str) -> Option<String> {
    params
        .as_ref()
        .and_then(|p| p.get(key))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn open_thread(params: &Option<Value>, svc: &ServiceState) -> Result<Value, JsonRpcError> {
    let invalid = |message: String| JsonRpcError {
        code: -32602,
        message,
        data: None,
    };
    let service = thread_param(params, "service")
        .unwrap_or_else(|| codeterm_bridge::bridge::threads::SERVICE.to_string());
    if service != codeterm_bridge::bridge::threads::SERVICE {
        return Err(invalid(format!("service '{service}' has no topic threads")));
    }
    let caller = thread_param(params, "caller_tab_id")
        .ok_or_else(|| invalid("caller_tab_id is required: run inside a Domios tab".to_string()))?;
    let target = thread_param(params, "tab_id").unwrap_or_else(|| caller.clone());
    let chat_id = params
        .as_ref()
        .and_then(|p| p.get("chat_id"))
        .and_then(|v| v.as_i64());
    let svc = svc.clone();
    let opened = codeterm_service_core::async_runtime::block_on(async move {
        svc.open_bridge_thread(Some(&caller), &target, chat_id)
            .await
    })
    .map_err(|error| JsonRpcError {
        code: if error.is_authority() {
            THREAD_AUTHORITY_CODE
        } else {
            -32602
        },
        message: error.to_string(),
        data: None,
    })?;
    serde_json::to_value(opened).map_err(|error| JsonRpcError {
        code: -32603,
        message: error.to_string(),
        data: None,
    })
}
