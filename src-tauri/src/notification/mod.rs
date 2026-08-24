pub mod answer;
pub mod answer_delivery;
pub mod bell;
pub mod dedupe;
pub mod desktop;
pub mod push;
pub mod store;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::State;

use store::{Notification, NotificationStore};

/// Shared notification state, thread-safe.
pub type NotifState = Arc<Mutex<NotificationStore>>;

/// Create a notification. `level` names the closed set; omitting it keeps
/// the notification in the inbox instead of raising a desktop alert.
#[tauri::command]
pub fn notify(
    svc: State<'_, crate::service::ServiceState>,
    title: String,
    body: Option<String>,
    severity: Option<String>,
    tab_id: Option<String>,
    level: Option<String>,
    event_key: Option<String>,
) -> Result<String, String> {
    let intent =
        crate::api::notification_routes::parse_push_intent(level.as_deref(), event_key)?;
    Ok(svc.create_notification_ext(
        title,
        body,
        severity.as_deref(),
        None,
        None,
        crate::service::notification::NotificationOrigin::claimed(tab_id),
        intent,
    ))
}

/// Whether the Telegram push opt-in may be offered in settings at all.
#[tauri::command]
pub fn telegram_push_gate(
    svc: State<'_, crate::service::ServiceState>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "control_available": svc.telegram_push_control_available(),
    }))
}

/// List notifications, optionally filtered to unread only.
#[tauri::command]
pub fn list_notifications(
    store: State<'_, NotifState>,
    unread_only: Option<bool>,
) -> Result<Vec<Notification>, String> {
    let s = store.lock();
    let unread = unread_only.unwrap_or(false);
    Ok(s.list(unread).into_iter().cloned().collect())
}

/// Record the user's choice on a notification. The caller names the record and
/// the chosen label; it cannot name a command, arguments, or a target tab.
#[tauri::command]
pub fn answer_notification(
    svc: State<'_, crate::service::ServiceState>,
    id: String,
    label: String,
) -> Result<serde_json::Value, String> {
    svc.answer_notification(&id, &label)
}

/// Mark a single notification as read.
#[tauri::command]
pub fn mark_notification_read(store: State<'_, NotifState>, id: String) -> Result<(), String> {
    let mut s = store.lock();
    if s.mark_read(&id) {
        Ok(())
    } else {
        Err(format!("Notification not found: {}", id))
    }
}

/// Clear one notification by ID, or all if no ID given.
#[tauri::command]
pub fn clear_notifications(store: State<'_, NotifState>, id: Option<String>) -> Result<(), String> {
    let mut s = store.lock();
    s.clear(id.as_deref());
    Ok(())
}
