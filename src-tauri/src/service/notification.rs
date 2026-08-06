//! Notification creation with OS toast support.

use super::ServiceContext;
use crate::notification::{
    store::{DedupAdmission, NotificationLevel},
    NotifState,
};

#[derive(Debug)]
enum NotificationAdmission {
    Diagnostic,
    /// Explicitly requested (CLI/REST/frontend): stored, emitted, native best-effort.
    Explicit,
    ProviderUpdate {
        prompt_id: String,
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
        NotificationAdmission::Explicit => store.lock().add_ext(
            title.clone(),
            body.clone(),
            parsed_level,
            tab_id.clone(),
            workspace_id.clone(),
            actions.clone(),
            sticky,
        ),
        NotificationAdmission::ProviderUpdate { prompt_id } => {
            let mut store = store.lock();
            match store.add_provider_update(
                prompt_id,
                title.clone(),
                body.clone().unwrap_or_default(),
                actions.clone().unwrap_or_else(|| serde_json::json!([])),
                tab_id.clone().expect("provider update has a tab"),
                workspace_id.clone(),
            ) {
                DedupAdmission::Added(id) => id,
                DedupAdmission::Duplicate(id) => return id,
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
    pub(crate) fn create_provider_update_notification(
        &self,
        prompt_id: String,
        provider_name: &str,
        message: String,
        tab_id: String,
    ) -> String {
        let workspace_id = self.mux.read().find_workspace_for_tab(&tab_id);
        let actions = serde_json::json!([
            {
                "label": "Update",
                "type": "invoke",
                "value": "respond_provider_update_prompt",
                "args": { "promptId": prompt_id, "accept": true }
            },
            {
                "label": "Not now",
                "type": "invoke",
                "value": "respond_provider_update_prompt",
                "args": { "promptId": prompt_id, "accept": false }
            }
        ]);
        create_notification_with_delivery(
            NotificationAdmission::ProviderUpdate {
                prompt_id: prompt_id.clone(),
            },
            &self.notif,
            |_, _| {},
            format!("{provider_name} update available"),
            Some(message),
            Some("info"),
            Some(actions),
            Some(true),
            Some(tab_id),
            workspace_id,
            |payload| self.notify_event("notification-created", payload),
        )
    }

    pub(crate) fn resolve_provider_update_notification(&self, prompt_id: &str) {
        let resolved = self.notif.lock().resolve_provider_update(prompt_id);
        if let Some(id) = resolved {
            self.notify_event("notification-cleared", serde_json::json!({ "id": id }));
        }
    }

    /// Internal diagnostic sink: fire-and-forget, never enters the user inbox.
    pub fn create_notification(
        &self,
        title: String,
        body: Option<String>,
        level: Option<&str>,
    ) -> String {
        create_notification_with_delivery(
            NotificationAdmission::Diagnostic,
            &self.notif,
            |_, _| {},
            title,
            body,
            level,
            None,
            None,
            None,
            None,
            |_| {},
        )
    }

    /// Explicitly requested notification (CLI/REST/frontend surfaces).
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
            NotificationAdmission::Explicit,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::store::NotificationStore;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn explicit_notification_persists_emits_and_requests_native_delivery() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        let native = Mutex::new(Vec::new());

        let id = create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |title, body| {
                assert_eq!(
                    store.lock().list(false).len(),
                    1,
                    "durable history is written before the native sink runs"
                );
                native
                    .lock()
                    .push((title.to_string(), body.map(str::to_string)));
            },
            "Build finished".into(),
            Some("release artifacts ready".into()),
            Some("warning"),
            None,
            None,
            None,
            None,
            |payload| emitted.lock().push(payload),
        );

        let guard = store.lock();
        let listed = guard.list(false);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id, "returned id names the stored record");
        assert_eq!(listed[0].title, "Build finished");
        assert_eq!(listed[0].body.as_deref(), Some("release artifacts ready"));
        assert_eq!(listed[0].level, NotificationLevel::Warning);
        assert!(!listed[0].read);
        drop(guard);

        assert_eq!(
            native.lock().as_slice(),
            &[(
                "Build finished".to_string(),
                Some("release artifacts ready".to_string())
            )]
        );
        let events = emitted.lock();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], id);
    }

    #[test]
    fn native_sink_failure_does_not_drop_durable_history() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());

        // Denied permission / unsupported platform: the sink drops the toast.
        let id = create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &store,
            |_, _| {},
            "Escalation".into(),
            Some("user attention required".into()),
            Some("error"),
            None,
            None,
            None,
            None,
            |payload| emitted.lock().push(payload),
        );

        let mut guard = store.lock();
        assert_eq!(guard.list(false).len(), 1);
        assert_eq!(guard.list(false)[0].id, id);
        assert_eq!(emitted.lock().len(), 1, "bell route still fires");

        guard.clear(Some(&id));
        assert!(
            guard.list(false).is_empty(),
            "explicit clear is the only way the record leaves history"
        );
    }

    #[test]
    fn diagnostic_callers_never_enter_notification_history() {
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
            "service/output.rs",
            "service/tab.rs",
            "service/spawn_chatbackend.rs",
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
