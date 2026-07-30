//! Notification creation with OS toast support.

use super::ServiceContext;
use crate::notification::{
    store::{AgentInputAdmission, NotificationLevel},
    NotifState,
};
use codeterm_types::pty::{QuestionSubtype, TimelineState};

#[derive(Debug)]
enum NotificationAdmission {
    Diagnostic,
    AgentInputRequest {
        session_id: String,
        prompt_identity: String,
    },
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
    deliver_native: impl FnOnce(&str, Option<&str>),
    title: String,
    body: Option<String>,
    level: Option<&str>,
    actions: Option<serde_json::Value>,
    sticky: Option<bool>,
    tab_id: Option<String>,
    workspace_id: Option<String>,
    emit: impl FnOnce(serde_json::Value),
) -> String {
    if matches!(admission, NotificationAdmission::Diagnostic) {
        return uuid::Uuid::new_v4().to_string();
    }
    let parsed_level = notification_level(level);
    let level_str = match &parsed_level {
        NotificationLevel::Warning => "Warning",
        NotificationLevel::Error => "Error",
        _ => "Info",
    };
    let id = match admission {
        NotificationAdmission::Diagnostic => unreachable!(),
        NotificationAdmission::AgentInputRequest {
            session_id,
            prompt_identity,
        } => {
            let mut store = store.lock();
            match store.add_agent_input_request(
                tab_id.clone().expect("agent input request has a tab"),
                session_id,
                prompt_identity,
                title.clone(),
                body.clone().expect("agent input request has a body"),
                workspace_id
                    .clone()
                    .expect("agent input request has a workspace"),
            ) {
                AgentInputAdmission::Added(id) => id,
                AgentInputAdmission::Duplicate(id) => return id,
            }
        }
    };

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
    deliver_native(
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
            |title, body| {
                crate::api::os_notify::_send_os_notification(&self.app_handle, title, body)
            },
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
        prompt_identity: Option<String>,
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
        let prompt_identity = prompt_identity
            .map(|identity| identity.trim().to_string())
            .filter(|identity| !identity.is_empty())
            .unwrap_or_else(|| body.clone());
        if prompt_identity != body {
            return Err("Prompt is no longer authoritative".to_string());
        }

        Ok(create_notification_with_delivery(
            NotificationAdmission::AgentInputRequest {
                session_id,
                prompt_identity,
            },
            &self.notif,
            |title, body| {
                crate::api::os_notify::_send_os_notification(&self.app_handle, title, body)
            },
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

    pub fn resolve_agent_input_request_notification(
        &self,
        tab_id: &str,
        session_id: &str,
        prompt_identity: &str,
    ) {
        self.notif
            .lock()
            .resolve_agent_input_request(tab_id, session_id, prompt_identity.trim());
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
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        let native = Mutex::new(Vec::new());

        let id = create_notification_with_delivery(
            NotificationAdmission::AgentInputRequest {
                session_id: "session-owned".into(),
                prompt_identity: "Which deployment region should I use?".into(),
            },
            &store,
            |title, body| {
                native
                    .lock()
                    .push((title.to_string(), body.map(str::to_string)))
            },
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

        assert_eq!(native.lock().len(), 1);
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
            events[0]["title"].as_str().unwrap().contains("Tab"),
            "user-visible notification copy uses Tab wording"
        );
    }

    #[test]
    fn replayed_input_request_skips_every_delivery_channel() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        let native = Mutex::new(Vec::new());

        for prompt in [
            "Which deployment region?",
            "Which deployment region?",
            "Choose a failover region",
        ] {
            create_notification_with_delivery(
                NotificationAdmission::AgentInputRequest {
                    session_id: "session-owned".into(),
                    prompt_identity: prompt.into(),
                },
                &store,
                |title, body| {
                    native
                        .lock()
                        .push((title.to_string(), body.map(str::to_string)))
                },
                "Input needed in Tab Review".into(),
                Some(prompt.into()),
                Some("warning"),
                None,
                None,
                Some("tab-owned".into()),
                Some("workspace-owned".into()),
                |payload| emitted.lock().push(payload),
            );
        }

        assert_eq!(store.lock().list(false).len(), 2);
        assert_eq!(native.lock().len(), 2);
        assert_eq!(emitted.lock().len(), 2);
    }

    #[test]
    fn diagnostic_callers_never_enter_the_agent_input_inbox() {
        let store = Arc::new(Mutex::new(NotificationStore::new(20)));
        let emitted = Mutex::new(Vec::new());
        let native = Mutex::new(Vec::new());
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
                |title, body| {
                    native
                        .lock()
                        .push((title.to_string(), body.map(str::to_string)))
                },
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
        assert_eq!(native.lock().len(), 0);
    }
}
