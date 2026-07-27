//! Notification creation with OS toast support.

use super::ServiceContext;
use crate::notification::{store::NotificationLevel, NotifState};
use codeterm_types::pty::{QuestionSubtype, TimelineState};
use tauri::AppHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotificationAdmission {
    Diagnostic,
    AgentInputRequest,
}

fn notification_level(level: Option<&str>) -> NotificationLevel {
    match level {
        Some("warning") | Some("Warning") => NotificationLevel::Warning,
        Some("error") | Some("Error") => NotificationLevel::Error,
        _ => NotificationLevel::Info,
    }
}

fn create_notification_with_delivery(
    admission: NotificationAdmission,
    store: &NotifState,
    app: &AppHandle,
    title: String,
    body: Option<String>,
    level: Option<&str>,
    actions: Option<serde_json::Value>,
    sticky: Option<bool>,
    tab_id: Option<String>,
    workspace_id: Option<String>,
    emit: impl FnOnce(serde_json::Value),
) -> String {
    if admission == NotificationAdmission::Diagnostic {
        return uuid::Uuid::new_v4().to_string();
    }
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
        tab_id.clone(),
        workspace_id.clone(),
    );

    let mut payload = serde_json::json!({
        "id": id,
        "title": title,
        "body": body,
        "level": level_str,
        "tab_id": tab_id,
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
        tab_id: Option<String>,
        workspace_id: Option<String>,
    ) -> String {
        create_notification_with_delivery(
            NotificationAdmission::Diagnostic,
            &self.notif,
            &self.app_handle,
            title,
            body,
            level,
            actions,
            sticky,
            tab_id,
            workspace_id,
            |payload| self.notify_event("notification-created", payload),
        )
    }

    pub fn create_agent_input_request_notification(
        &self,
        tab_id: String,
        session_id: String,
    ) -> Result<String, String> {
        let (title, body, workspace_id) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(&tab_id)
                .ok_or_else(|| "Tab not found".to_string())?;
            if tab.agent_session_id.as_deref() != Some(session_id.as_str()) {
                return Err("Tab session is no longer authoritative".to_string());
            }
            let state = tab
                .agent_state
                .as_ref()
                .ok_or_else(|| "Tab is not waiting for input".to_string())?;
            if state.state != TimelineState::Question
                || matches!(
                    state.subtype,
                    Some(QuestionSubtype::RateLimit | QuestionSubtype::Error)
                )
            {
                return Err("Tab is not waiting for user input".to_string());
            }
            let body = tab
                .pending_prompt
                .as_ref()
                .map(|prompt| prompt.question.trim())
                .filter(|message| !message.is_empty())
                .or_else(|| {
                    state
                        .question
                        .as_deref()
                        .map(str::trim)
                        .filter(|message| !message.is_empty())
                })
                .ok_or_else(|| "Complete input request is not available".to_string())?
                .to_string();
            let workspace_id = mux
                .find_workspace_for_tab(&tab_id)
                .ok_or_else(|| "Owning workspace not found".to_string())?;
            (
                format!("Input needed in Tab {}", tab.title),
                body,
                workspace_id,
            )
        };

        Ok(create_notification_with_delivery(
            NotificationAdmission::AgentInputRequest,
            &self.notif,
            &self.app_handle,
            title,
            Some(body),
            Some("warning"),
            None,
            None,
            Some(tab_id),
            Some(workspace_id),
            |payload| self.notify_event("notification-created", payload),
        ))
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
            NotificationAdmission::AgentInputRequest,
            &store,
            app.handle(),
            "Input needed in Tab Review".into(),
            Some("Which deployment region should I use?\nInclude the failover region.".into()),
            Some("warning"),
            None,
            None,
            Some("tab-owned".into()),
            Some("workspace-owned".into()),
            |payload| emitted.lock().push(payload),
        );

        let notifications = store.lock();
        assert_eq!(notifications.list(false).len(), 1);
        let notification = notifications.list(false)[0];
        assert_eq!(notification.id, id);
        assert_eq!(notification.tab_id.as_deref(), Some("tab-owned"));
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
        assert_eq!(events[0]["tab_id"], "tab-owned");
        assert_eq!(events[0]["workspace_id"], "workspace-owned");
        assert_eq!(
            events[0]["body"],
            "Which deployment region should I use?\nInclude the failover region."
        );
        assert!(
            !events[0]["title"].as_str().unwrap().contains("Tab"),
            "user-visible notification copy uses Tab wording"
        );
    }

    #[test]
    fn diagnostic_callers_never_enter_the_agent_input_inbox() {
        let builder = tauri::Builder::default();
        #[cfg(windows)]
        let builder = builder.any_thread();
        let app = builder
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let store = Arc::new(Mutex::new(NotificationStore::new(20)));
        let emitted = Mutex::new(Vec::new());
        let native_before = crate::api::os_notify::os_notification_test_count();
        let sources = [
            "service/spawn.rs:289",
            "service/spawn.rs:703",
            "cron/notify.rs",
            "cron/scheduler.rs",
            "service/orchestrator/refresh/driver.rs",
            "service/orchestrator/batch.rs",
            "api/notification_routes.rs",
            "service/output.rs",
            "service/tab.rs",
            "service/spawn_chatbackend.rs",
            "socket/server/notification.rs",
        ];

        for source in sources {
            create_notification_with_delivery(
                NotificationAdmission::Diagnostic,
                &store,
                app.handle(),
                format!("Diagnostic from {source}"),
                Some("warning or error detail".into()),
                Some("warning"),
                None,
                None,
                None,
                None,
                |payload| emitted.lock().push(payload),
            );
        }

        assert_eq!(store.lock().list(false).len(), 0);
        assert_eq!(emitted.lock().len(), 0);
        assert_eq!(
            crate::api::os_notify::os_notification_test_count(),
            native_before
        );
    }
}
