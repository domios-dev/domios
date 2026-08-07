//! Notification creation with OS toast support.

use super::ServiceContext;
use crate::notification::{
    store::{DedupAdmission, NotificationLevel, NotificationProvenance},
    NotifState,
};

/// Untrusted navigation hints attached to a notification request. `caller_tab_id`
/// is the transport-authenticated caller, `claimed_tab_id` is payload-supplied.
#[derive(Debug, Clone, Default)]
pub struct NotificationOrigin {
    pub caller_tab_id: Option<String>,
    pub claimed_tab_id: Option<String>,
    pub peer_name: Option<String>,
}

impl NotificationOrigin {
    pub fn claimed(tab_id: Option<String>) -> Self {
        Self {
            claimed_tab_id: tab_id,
            ..Default::default()
        }
    }

    pub fn local_tab(tab_id: String) -> Self {
        Self {
            caller_tab_id: Some(tab_id),
            ..Default::default()
        }
    }
}

/// Resolve an origin to daemon-owned provenance. A remote peer never names a
/// local tab, the authenticated caller outranks any payload claim, and an
/// unresolvable id yields empty provenance rather than a stored navigation target.
fn resolve_origin(
    origin: &NotificationOrigin,
    resolve_tab: impl FnOnce(&str) -> Option<NotificationProvenance>,
) -> NotificationProvenance {
    if let Some(peer_name) = origin.peer_name.as_deref() {
        return NotificationProvenance {
            peer_name: Some(peer_name.to_string()),
            ..Default::default()
        };
    }
    origin
        .caller_tab_id
        .as_deref()
        .or(origin.claimed_tab_id.as_deref())
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .and_then(resolve_tab)
        .unwrap_or_default()
}

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
    origin: NotificationOrigin,
    resolve_tab: impl FnOnce(&str) -> Option<NotificationProvenance>,
    emit: impl FnOnce(serde_json::Value),
) -> String {
    if matches!(admission, NotificationAdmission::Diagnostic) {
        return uuid::Uuid::new_v4().to_string();
    }
    let provenance = resolve_origin(&origin, resolve_tab);
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
            provenance.clone(),
            actions.clone(),
            sticky,
        ),
        NotificationAdmission::ProviderUpdate { prompt_id } => {
            // The prompt's only action targets its tab; a tab that no longer
            // resolves leaves nothing to navigate to or answer.
            if provenance.tab_id.is_none() {
                return uuid::Uuid::new_v4().to_string();
            }
            let mut store = store.lock();
            match store.add_provider_update(
                prompt_id,
                title.clone(),
                body.clone().unwrap_or_default(),
                actions.clone().unwrap_or_else(|| serde_json::json!([])),
                provenance.clone(),
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
        "tab_id": provenance.tab_id.clone(),
        "workspace_id": provenance.workspace_id.clone(),
        "tab_title": provenance.tab_title.clone(),
        "peer_name": provenance.peer_name.clone(),
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
    /// Live-multiplexer lookup: a tab id only becomes provenance if the tab is
    /// still open, and its title and workspace come from the tab, not the caller.
    fn resolve_tab_provenance(&self, tab_id: &str) -> Option<NotificationProvenance> {
        let mux = self.mux.read();
        let tab_title = mux.get_tab(tab_id).map(|tab| tab.title.clone())?;
        Some(NotificationProvenance {
            tab_id: Some(tab_id.to_string()),
            tab_title: Some(tab_title),
            workspace_id: mux.find_workspace_for_tab(tab_id),
            peer_name: None,
        })
    }

    pub(crate) fn create_provider_update_notification(
        &self,
        prompt_id: String,
        provider_name: &str,
        message: String,
        tab_id: String,
    ) -> String {
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
            NotificationOrigin::local_tab(tab_id),
            |candidate| self.resolve_tab_provenance(candidate),
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
            NotificationOrigin::default(),
            |_| None,
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
        origin: NotificationOrigin,
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
            origin,
            |candidate| self.resolve_tab_provenance(candidate),
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
            NotificationOrigin::default(),
            |_| None,
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
            NotificationOrigin::default(),
            |_| None,
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
                NotificationOrigin::default(),
                |_| None,
                |payload| emitted.lock().push(payload),
            );
        }

        assert_eq!(store.lock().list(false).len(), 0);
        assert_eq!(emitted.lock().len(), 0);
        assert_eq!(native.lock().len(), 0);
    }

    mod notification_provenance {
        use super::*;

        /// Stand-in for the live multiplexer: only ids present here resolve, and
        /// title plus workspace come from the registry, never from the caller.
        const LIVE_TABS: [(&str, &str, &str); 2] = [
            ("tab_caller", "Worker lane", "ws_lanes"),
            ("tab_bystander", "Unrelated shell", "ws_scratch"),
        ];

        fn live_registry(candidate: &str) -> Option<NotificationProvenance> {
            LIVE_TABS
                .iter()
                .find(|(id, _, _)| *id == candidate)
                .map(|(id, title, workspace)| NotificationProvenance {
                    tab_id: Some((*id).to_string()),
                    tab_title: Some((*title).to_string()),
                    workspace_id: Some((*workspace).to_string()),
                    peer_name: None,
                })
        }

        #[test]
        fn unresolvable_claimed_tab_is_dropped_not_stored() {
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let emitted = Mutex::new(Vec::new());
            assert!(
                live_registry("tab_bystander").is_some(),
                "the fixture keeps an unrelated live tab so an empty registry cannot short-circuit"
            );

            let id = create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Attention".into(),
                Some("agent needs input".into()),
                Some("warning"),
                None,
                None,
                NotificationOrigin::claimed(Some("tab_never_existed".into())),
                live_registry,
                |payload| emitted.lock().push(payload),
            );

            let guard = store.lock();
            let listed = guard.list(false);
            assert_eq!(listed.len(), 1, "the notification still lands");
            assert_eq!(listed[0].id, id);
            assert_eq!(listed[0].tab_id, None, "the claim is dropped, not stored");
            assert_eq!(listed[0].workspace_id, None);
            assert_eq!(listed[0].tab_title, None);
            drop(guard);

            let events = emitted.lock();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["tab_id"], serde_json::Value::Null);
        }

        #[test]
        fn resolved_tab_overrides_a_conflicting_claim() {
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let emitted = Mutex::new(Vec::new());
            let origin = NotificationOrigin {
                caller_tab_id: Some("tab_caller".into()),
                claimed_tab_id: Some("tab_bystander".into()),
                peer_name: None,
            };
            assert_ne!(
                origin.caller_tab_id, origin.claimed_tab_id,
                "claim and resolution must differ for this assertion to mean anything"
            );
            assert!(
                live_registry("tab_bystander").is_some(),
                "the losing claim is itself a live tab, so only precedence can decide"
            );

            create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Attention".into(),
                None,
                Some("info"),
                None,
                None,
                origin,
                live_registry,
                |payload| emitted.lock().push(payload),
            );

            let guard = store.lock();
            let listed = guard.list(false);
            assert_eq!(listed[0].tab_id.as_deref(), Some("tab_caller"));
            assert_eq!(listed[0].tab_title.as_deref(), Some("Worker lane"));
            assert_eq!(listed[0].workspace_id.as_deref(), Some("ws_lanes"));
            drop(guard);

            assert_eq!(emitted.lock()[0]["tab_id"], "tab_caller");
        }

        #[test]
        fn mesh_peer_origin_never_stores_a_tab() {
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let origin = NotificationOrigin {
                caller_tab_id: None,
                claimed_tab_id: Some("tab_caller".into()),
                peer_name: Some("BLACKSTAR".into()),
            };

            create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Remote build finished".into(),
                None,
                Some("info"),
                None,
                None,
                origin,
                live_registry,
                |_| {},
            );

            let guard = store.lock();
            let listed = guard.list(false);
            assert_eq!(listed[0].tab_id, None);
            assert_eq!(listed[0].workspace_id, None);
            assert_eq!(listed[0].peer_name.as_deref(), Some("BLACKSTAR"));
        }
    }
}
