pub mod store;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::State;

use store::{Notification, NotificationLevel, NotificationStore};

/// Shared notification state, thread-safe.
pub type NotifState = Arc<Mutex<NotificationStore>>;

/// Create a notification.
#[tauri::command]
pub fn notify(
    store: State<'_, NotifState>,
    title: String,
    body: Option<String>,
    level: Option<String>,
    pane_id: Option<String>,
    workspace_id: Option<String>,
) -> Result<String, String> {
    let level = match level.as_deref() {
        Some("warning") | Some("Warning") => NotificationLevel::Warning,
        Some("error") | Some("Error") => NotificationLevel::Error,
        _ => NotificationLevel::Info,
    };

    let mut s = store.lock();
    let id = s.add(title, body, level, pane_id, workspace_id);
    Ok(id)
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
pub fn mark_notification_read(
    store: State<'_, NotifState>,
    id: String,
) -> Result<(), String> {
    let mut s = store.lock();
    if s.mark_read(&id) {
        Ok(())
    } else {
        Err(format!("Notification not found: {}", id))
    }
}

/// Clear one notification by ID, or all if no ID given.
#[tauri::command]
pub fn clear_notifications(
    store: State<'_, NotifState>,
    id: Option<String>,
) -> Result<(), String> {
    let mut s = store.lock();
    s.clear(id.as_deref());
    Ok(())
}
