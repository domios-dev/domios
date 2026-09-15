//! `bridge.*` JSON-RPC handlers (Telegram outbound + reactions + restart).

use serde_json::Value;

use crate::service::ServiceState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "bridge.restart" => {
            crate::spawn_telegram_bridge(svc.clone());
            Ok(serde_json::json!({"ok": true, "message": "Telegram bridge starting"}))
        }

        "bridge.send" => {
            let service = params
                .as_ref()
                .and_then(|p| p.get("service"))
                .and_then(|v| v.as_str())
                .unwrap_or("telegram");
            crate::bridge::manager::ensure_supported_service(service).map_err(|msg| {
                JsonRpcError {
                    code: -32602,
                    message: msg.to_string(),
                    data: None,
                }
            })?;

            let chat_id = params
                .as_ref()
                .and_then(|p| p.get("chat_id"))
                .and_then(|v| v.as_i64());
            let append = params
                .as_ref()
                .and_then(|p| p.get("append"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let content_type = params
                .as_ref()
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("text")
                .to_string();

            let content = match content_type.as_str() {
                "text" => {
                    let text = get_string_param(params, "text")?;
                    crate::bridge::traits::OutboundContent::Text { text }
                }
                "photo" | "document" | "voice" | "animation" | "video" | "sticker" => {
                    let file = get_string_param(params, "file")?;
                    if !std::path::Path::new(&file).exists() {
                        return Err(JsonRpcError {
                            code: -32602,
                            message: format!("File not found: {}", file),
                            data: None,
                        });
                    }
                    let caption = params
                        .as_ref()
                        .and_then(|p| p.get("caption"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            params
                                .as_ref()
                                .and_then(|p| p.get("text"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        });
                    let filename = params
                        .as_ref()
                        .and_then(|p| p.get("filename"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    match content_type.as_str() {
                        "photo" => crate::bridge::traits::OutboundContent::Photo { file, caption },
                        "document" => crate::bridge::traits::OutboundContent::Document {
                            file,
                            caption,
                            filename,
                        },
                        "voice" => crate::bridge::traits::OutboundContent::Voice { file, caption },
                        "animation" => {
                            crate::bridge::traits::OutboundContent::Animation { file, caption }
                        }
                        "video" => crate::bridge::traits::OutboundContent::Video { file, caption },
                        "sticker" => crate::bridge::traits::OutboundContent::Sticker { file },
                        _ => unreachable!(),
                    }
                }
                _ => {
                    return Err(JsonRpcError {
                        code: -32602,
                        message: format!("Unknown content type: {}", content_type),
                        data: None,
                    });
                }
            };

            let buttons: Option<crate::bridge::traits::InlineKeyboard> = params
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
                .map(|tab_id| crate::bridge::manager::OutboundRoute::Tab { tab_id })
                .unwrap_or_default();

            if let Some(tx) = svc.bridge_outbound.lock().as_ref() {
                let msg = crate::bridge::manager::OutboundMessage {
                    user_id: chat_id,
                    content,
                    append,
                    buttons,
                    edit: None,
                    receipt: None,
                    route,
                };
                tx.send(msg).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: format!("Bridge send failed: {}", e),
                    data: None,
                })?;
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

            let token =
                crate::bridge::telegram::read_token(&crate::bridge_host::AppAttachmentStorePort)
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
    let caller = thread_param(params, "caller_tab_id").ok_or_else(|| {
        invalid("caller_tab_id is required: run inside a CodeTerm tab".to_string())
    })?;
    let target = thread_param(params, "tab_id").unwrap_or_else(|| caller.clone());
    let chat_id = params
        .as_ref()
        .and_then(|p| p.get("chat_id"))
        .and_then(|v| v.as_i64());
    let svc = svc.clone();
    let opened = tauri::async_runtime::block_on(async move {
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
