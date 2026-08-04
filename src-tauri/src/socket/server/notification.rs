//! `notification.*` JSON-RPC handlers.

use crate::service::ServiceState;
use serde_json::Value;

use super::{get_string_param, JsonRpcError};

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
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
            let tab_id = params
                .as_ref()
                .and_then(|p| p.get("tab_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let workspace_id = params
                .as_ref()
                .and_then(|p| p.get("workspace_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let id = svc.create_notification_ext(
                title,
                body,
                Some(level_str),
                None,
                None,
                tab_id,
                workspace_id,
            );

            Ok(serde_json::json!({"id": id}))
        }

        "notification.list" => {
            let unread_only = params
                .as_ref()
                .and_then(|p| p.get("unread_only"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let store = svc.notif.lock();
            let notifications: Vec<&crate::notification::store::Notification> =
                store.list(unread_only);
            serde_json::to_value(&notifications).map_err(|e| JsonRpcError {
                code: -32603,
                message: format!("Serialization error: {}", e),
                data: None,
            })
        }

        "notification.clear" => {
            let id = params
                .as_ref()
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str());

            let mut store = svc.notif.lock();
            store.clear(id);
            Ok(serde_json::json!({"ok": true}))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
