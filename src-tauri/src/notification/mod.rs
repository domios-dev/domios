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

/// Create a notification. `category` names the closed set; omitting it keeps
/// the notification in the inbox instead of raising a desktop alert.
#[tauri::command]
pub fn notify(
    svc: State<'_, crate::service::ServiceState>,
    title: String,
    body: Option<String>,
    level: Option<String>,
    tab_id: Option<String>,
    category: Option<String>,
    event_key: Option<String>,
) -> Result<String, String> {
    let intent = crate::api::notification_routes::parse_push_intent(category.as_deref(), event_key)
        .map_err(str::to_string)?;
    Ok(svc.create_notification_ext(
        title,
        body,
        level.as_deref(),
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
