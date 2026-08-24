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
            let severity_str = params
                .as_ref()
                .and_then(|p| p.get("severity"))
                .and_then(|v| v.as_str())
                .unwrap_or("info");
            let string_param = |key: &str| {
                params
                    .as_ref()
                    .and_then(|p| p.get(key))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            };
            // Both ids are caller-supplied hints; the daemon resolves them and
            // drops whatever does not name a live tab. `workspace_id` is derived.
            let origin = crate::service::notification::NotificationOrigin {
                caller_tab_id: string_param("caller_tab_id"),
                claimed_tab_id: string_param("tab_id"),
                peer_name: None,
            };
            // The CLI reaches the daemon here, so `--choice` is parsed on this
            // path too — an unknown kind is refused rather than dropped.
            let actions = match params.as_ref().and_then(|p| p.get("actions")) {
                None | Some(Value::Null) => None,
                Some(raw) => Some(
                    serde_json::from_value::<Vec<codeterm_types::notification::NotificationAction>>(
                        raw.clone(),
                    )
                    .map_err(|e| JsonRpcError {
                        code: -32602,
                        message: format!("Invalid notification actions: {e}"),
                        data: None,
                    })?,
                ),
            };
            let actions = crate::api::notification_routes::vet_actions(actions).map_err(
                |message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                },
            )?;
            let level = string_param("level");
            let intent = crate::api::notification_routes::parse_push_intent(
                level.as_deref(),
                string_param("event_key"),
            )
            .map_err(|message| JsonRpcError {
                code: -32602,
                message,
                data: None,
            })?;
            let id = svc.create_notification_ext(
                title,
                body,
                Some(severity_str),
                actions,
                None,
                origin,
                intent,
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
