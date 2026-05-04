//! `notification.*` JSON-RPC handlers.

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::notification::store::NotificationLevel;
use crate::notification::NotifState;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    notif: &NotifState,
    app: &AppHandle,
) -> Result<Value, JsonRpcError> {
    match method {
        "notification.create" => {
            let title = get_string_param(params, "title")?;
            let body = params
                .as_ref()
                .and_then(|p| p.get("body"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let level_str = params
                .as_ref()
                .and_then(|p| p.get("level"))
                .and_then(|v| v.as_str())
                .unwrap_or("info");
            let level = match level_str {
                "warning" => NotificationLevel::Warning,
                "error" => NotificationLevel::Error,
                _ => NotificationLevel::Info,
            };
            let pane_id = params
                .as_ref()
                .and_then(|p| p.get("pane_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let emit_level = match &level {
                NotificationLevel::Warning => "Warning",
                NotificationLevel::Error => "Error",
                _ => "Info",
            };
            let emit_title = title.clone();
            let emit_body = body.clone();
            let emit_pane = pane_id.clone();

            let mut store = notif.lock();
            let id = store.add(title, body, level, pane_id, None);

            { let h = app.clone(); let p = serde_json::json!({"id": id, "title": emit_title, "body": emit_body, "level": emit_level, "pane_id": emit_pane, "workspace_id": Value::Null}); std::thread::spawn(move || { let _ = h.emit("notification-created", p); }); }

            // OS notification if window unfocused
            {
                let is_focused = app
                    .get_webview_window("main")
                    .and_then(|w| w.is_focused().ok())
                    .unwrap_or(true);
                if !is_focused {
                    use tauri_plugin_notification::NotificationExt;
                    let mut builder = app.notification().builder()
                        .title(&emit_title);
                    if let Some(ref b) = emit_body {
                        builder = builder.body(b);
                    }
                    let _ = builder.show();
                }
            }

            Ok(serde_json::json!({"id": id}))
        }

        "notification.list" => {
            let unread_only = params
                .as_ref()
                .and_then(|p| p.get("unread_only"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let store = notif.lock();
            let notifications: Vec<&crate::notification::store::Notification> =
                store.list(unread_only);
            serde_json::to_value(&notifications).map_err(|e| JsonRpcError {
                code: -32603,
                message: format!("Serialization error: {}", e),
            })
        }

        "notification.clear" => {
            let id = params
                .as_ref()
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str());

            let mut store = notif.lock();
            store.clear(id);
            Ok(serde_json::json!({"ok": true}))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}
