//! Notification creation with OS toast support.

use crate::notification::store::NotificationLevel;
use super::ServiceContext;

impl ServiceContext {
    pub fn create_notification(
        &self,
        title: String,
        body: Option<String>,
        level: Option<&str>,
    ) -> String {
        self.create_notification_ext(title, body, level, None, None)
    }

    /// Extended notification with optional action buttons and sticky flag.
    pub fn create_notification_ext(
        &self,
        title: String,
        body: Option<String>,
        level: Option<&str>,
        actions: Option<serde_json::Value>,
        sticky: Option<bool>,
    ) -> String {
        let parsed_level = match level {
            Some("warning") => NotificationLevel::Warning,
            Some("error") => NotificationLevel::Error,
            _ => NotificationLevel::Info,
        };
        let level_str = match &parsed_level {
            NotificationLevel::Warning => "Warning",
            NotificationLevel::Error => "Error",
            _ => "Info",
        };
        let id = self
            .notif
            .lock()
            .add(title.clone(), body.clone(), parsed_level, None, None);

        let mut payload = serde_json::json!({
            "id": id,
            "title": title,
            "body": body,
            "level": level_str,
            "pane_id": serde_json::Value::Null,
            "workspace_id": serde_json::Value::Null,
        });
        if let Some(ref acts) = actions {
            payload["actions"] = acts.clone();
        }
        if let Some(s) = sticky {
            payload["sticky"] = serde_json::json!(s);
        }
        self.notify_event("notification-created", payload);
        id
    }
}
