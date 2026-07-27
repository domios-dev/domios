//! Notification creation with OS toast support.

use super::ServiceContext;
use crate::notification::{store::NotificationLevel, NotifState};
use tauri::AppHandle;

fn notification_level(level: Option<&str>) -> NotificationLevel {
    match level {
        Some("warning") | Some("Warning") => NotificationLevel::Warning,
        Some("error") | Some("Error") => NotificationLevel::Error,
        _ => NotificationLevel::Info,
    }
}

fn create_notification_with_delivery(
    store: &NotifState,
    app: &AppHandle,
    title: String,
    body: Option<String>,
    level: Option<&str>,
    actions: Option<serde_json::Value>,
    sticky: Option<bool>,
    pane_id: Option<String>,
    workspace_id: Option<String>,
    emit: impl FnOnce(serde_json::Value),
) -> String {
    let parsed_level = notification_level(level);
    let level_str = match &parsed_level {
        NotificationLevel::Warning => "Warning",
        NotificationLevel::Error => "Error",
        _ => "Info",
    };
    let id = store.lock().add(
        title.clone(),
        body.clone(),
        parsed_level,
        pane_id.clone(),
        workspace_id.clone(),
    );

    let mut payload = serde_json::json!({
        "id": id,
        "title": title,
        "body": body,
        "level": level_str,
        "pane_id": pane_id,
        "workspace_id": workspace_id,
    });
    if let Some(ref acts) = actions {
        payload["actions"] = acts.clone();
    }
    if let Some(s) = sticky {
        payload["sticky"] = serde_json::json!(s);
    }
    crate::api::os_notify::_send_os_notification(
        app,
        payload["title"].as_str().unwrap_or_default(),
        payload["body"].as_str(),
    );
    emit(payload);
    id
}

impl ServiceContext {
    pub fn create_notification(
        &self,
        title: String,
        body: Option<String>,
        level: Option<&str>,
    ) -> String {
        self.create_notification_ext(title, body, level, None, None, None, None)
    }

    /// Extended notification with optional action buttons and sticky flag.
    pub fn create_notification_ext(
        &self,
        title: String,
        body: Option<String>,
        level: Option<&str>,
        actions: Option<serde_json::Value>,
        sticky: Option<bool>,
        pane_id: Option<String>,
        workspace_id: Option<String>,
    ) -> String {
        create_notification_with_delivery(
            &self.notif,
            &self.app_handle,
            title,
            body,
            level,
            actions,
            sticky,
            pane_id,
            workspace_id,
            |payload| self.notify_event("notification-created", payload),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::store::NotificationStore;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn create_notification_delivers_bell_route_event_and_native_os_notification() {
        let builder = tauri::Builder::default();
        #[cfg(windows)]
        let builder = builder.any_thread();
        let app = builder
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        let native_before = crate::api::os_notify::os_notification_test_count();

        let id = create_notification_with_delivery(
            &store,
            app.handle(),
            "Waiting for you".into(),
            Some("Your agent needs input to continue.".into()),
            Some("warning"),
            None,
            None,
            Some("pane-owned".into()),
            Some("workspace-owned".into()),
            |payload| emitted.lock().push(payload),
        );

        let notifications = store.lock();
        assert_eq!(notifications.list(false).len(), 1);
        let notification = notifications.list(false)[0];
        assert_eq!(notification.id, id);
        assert_eq!(notification.pane_id.as_deref(), Some("pane-owned"));
        assert_eq!(
            notification.workspace_id.as_deref(),
            Some("workspace-owned")
        );
        drop(notifications);

        assert_eq!(
            crate::api::os_notify::os_notification_test_count(),
            native_before + 1
        );
        let events = emitted.lock();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], id);
        assert_eq!(events[0]["pane_id"], "pane-owned");
        assert_eq!(events[0]["workspace_id"], "workspace-owned");
    }
}
