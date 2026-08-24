//! Notification creation with OS toast support.

use codeterm_types::notification::NotificationAction;

use super::ServiceContext;
use crate::notification::{
    answer::AnswerAdmission,
    answer_delivery::{self, AnswerDeliveryOutcome},
    bell::{self, BellPolicy},
    dedupe::DeliveryAdmission,
    desktop::{self, DesktopPolicy},
    push::{self, NotifyLevel},
    store::{DedupAdmission, NotificationProvenance, NotificationSeverity, PushRequest},
    NotifState,
};

/// Caller-side push intent. `level` is the closed enum, so routine progress
/// has nothing to name; `event_key` is a hint the daemon derives when absent.
#[derive(Debug, Clone, Default)]
pub struct PushIntent {
    pub level: Option<NotifyLevel>,
    pub event_key: Option<String>,
}

/// What a push sink receives. Its `Result` is consumed by the daemon and never
/// travels back to the caller — failure feedback invites retry, and retry is
/// the spam vector.
pub struct PushEnvelope {
    pub level: NotifyLevel,
    pub text: String,
    pub notification_id: String,
}

/// Intent plus the daemon-owned policy and sink for one notification.
pub struct PushDispatch<F> {
    pub intent: PushIntent,
    pub per_scope_limit: u32,
    pub deliver: F,
}

fn inert_sink(_: PushEnvelope) -> Result<(), String> {
    Ok(())
}

impl PushDispatch<fn(PushEnvelope) -> Result<(), String>> {
    /// No opt-in, no audience, or no level: behaves exactly as before.
    pub fn inert() -> Self {
        Self {
            intent: PushIntent::default(),
            per_scope_limit: 0,
            deliver: inert_sink,
        }
    }
}

/// The two in-app legs of one notification: what the caller declared, and the
/// user's standing policy for each surface it may reach. Deliberately not
/// folded into `PushDispatch` — the push opt-in clears its own intent when
/// nobody is subscribed, while the declared level still governs the toast and
/// the bell.
pub struct SurfaceDispatch {
    pub declared: Option<NotifyLevel>,
    pub bell: BellPolicy,
    pub desktop: DesktopPolicy,
}

impl SurfaceDispatch {
    /// Routes that raise no toast at all: internal diagnostics and the in-app
    /// provider-update prompt. They still reach the bell, which is where they
    /// have always landed — silence is about interrupting, not about recording.
    pub fn silent() -> Self {
        Self {
            declared: None,
            bell: BellPolicy::open(),
            desktop: DesktopPolicy::silent(),
        }
    }
}

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

#[cfg(test)]
mod notify_gate_tests;

#[derive(Debug)]
enum NotificationAdmission {
    Diagnostic,
    /// Explicitly requested (CLI/REST/frontend): stored, emitted, native best-effort.
    Explicit,
    ProviderUpdate {
        prompt_id: String,
    },
}

fn notification_severity(severity: Option<&str>) -> NotificationSeverity {
    match severity {
        Some("warning") | Some("Warning") => NotificationSeverity::Warning,
        Some("error") | Some("Error") => NotificationSeverity::Error,
        _ => NotificationSeverity::Info,
    }
}

#[allow(clippy::too_many_arguments)]
fn create_notification_with_delivery(
    admission: NotificationAdmission,
    store: &NotifState,
    deliver_native: impl FnOnce(&str, Option<&str>),
    title: String,
    body: Option<String>,
    severity: Option<&str>,
    actions: Option<Vec<NotificationAction>>,
    sticky: Option<bool>,
    origin: NotificationOrigin,
    resolve_tab: impl FnOnce(&str) -> Option<NotificationProvenance>,
    push: PushDispatch<impl FnOnce(PushEnvelope) -> Result<(), String>>,
    surfaces: SurfaceDispatch,
    emit: impl FnOnce(serde_json::Value),
) -> String {
    if matches!(admission, NotificationAdmission::Diagnostic) {
        return uuid::Uuid::new_v4().to_string();
    }
    let provenance = resolve_origin(&origin, resolve_tab);
    let parsed_severity = notification_severity(severity);
    let severity_str = match &parsed_severity {
        NotificationSeverity::Warning => "Warning",
        NotificationSeverity::Error => "Error",
        _ => "Info",
    };
    let id = match admission {
        NotificationAdmission::Diagnostic => unreachable!(),
        NotificationAdmission::Explicit => match push.intent.level {
            // An identical live message is the same message: it keeps its
            // record and reaches neither the native sink nor the bell again.
            // Push routing may clear its own intent (no audience); the
            // caller's declared level still stands and is what gets stored.
            None => match store.lock().add_deduped(
                title.clone(),
                body.clone(),
                parsed_severity,
                provenance.clone(),
                actions.clone(),
                sticky,
                surfaces.declared,
            ) {
                DeliveryAdmission::Deliver(id) => id,
                DeliveryAdmission::Duplicate(id) => return id,
            },
            Some(level) => {
                let admitted = store.lock().add_push(PushRequest {
                    scope: push::scope_key(&provenance),
                    level,
                    event_key: push::event_key(push.intent.event_key.as_deref(), &title),
                    title: title.clone(),
                    body: body.clone(),
                    severity: parsed_severity,
                    provenance: provenance.clone(),
                    per_scope_limit: push.per_scope_limit,
                });
                // A duplicate reuses an open record: nothing new to toast or emit.
                if !admitted.opened_or_changed_record() {
                    return admitted.id().to_string();
                }
                if admitted.should_push() {
                    let envelope = PushEnvelope {
                        level,
                        text: push::render_push(level, &title, body.as_deref(), &provenance),
                        notification_id: admitted.id().to_string(),
                    };
                    if let Err(reason) = (push.deliver)(envelope) {
                        log::warn!(
                            "[notify-push] delivery failed for {}: {reason}",
                            admitted.id()
                        );
                        store.lock().mark_push_failed(admitted.id());
                    }
                }
                admitted.id().to_string()
            }
        },
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
                actions.clone().unwrap_or_default(),
                provenance.clone(),
            ) {
                DedupAdmission::Added(id) => id,
                DedupAdmission::Duplicate(id) => return id,
            }
        }
    };

    // The answer slot the store just built, so every surface renders the same
    // capture rather than deriving its own.
    let answer = store.lock().answer_slot(&id).cloned();
    // Read back off the record rather than reusing `surfaces.declared`: the
    // payload must always agree with what was actually stored.
    let level = store.lock().level(&id);
    let mut payload = serde_json::json!({
        "id": id,
        "title": title,
        "body": body,
        "severity": severity_str,
        "tab_id": provenance.tab_id.clone(),
        "workspace_id": provenance.workspace_id.clone(),
        "tab_title": provenance.tab_title.clone(),
        "peer_name": provenance.peer_name.clone(),
    });
    if let Some(ref acts) = actions {
        payload["actions"] = serde_json::to_value(acts).unwrap_or_default();
    }
    if let Some(ref slot) = answer {
        payload["answer"] = serde_json::to_value(slot).unwrap_or_default();
    }
    if let Some(level) = level {
        payload["level"] = serde_json::json!(level.as_str());
    }
    if let Some(s) = sticky {
        payload["sticky"] = serde_json::json!(s);
    }
    // Dedupe already decided this message is new; the gates decide where it may
    // go. Unlevelled keeps its record and its bell event, and stops.
    if let Some(pass) = desktop::admit(surfaces.declared, &surfaces.desktop).pass() {
        log::debug!("[notify-gate] desktop toast admitted as {}", pass.level().as_str());
        deliver_native(
            payload["title"].as_str().unwrap_or_default(),
            payload["body"].as_str(),
        );
    }
    // The bell column is routing: a level the user routed away from the bell
    // keeps its record and simply never arrives there. The master switch is a
    // separate, presentation-only decision and never reaches this gate.
    match bell::admit(surfaces.declared, &surfaces.bell) {
        bell::BellAdmission::Allowed => emit(payload),
        bell::BellAdmission::LevelDisabled(level) => {
            log::debug!("[notify-gate] bell refused {}", level.as_str());
        }
    }
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
        let respond = |label: &str, accept: bool| NotificationAction::Invoke {
            label: label.to_string(),
            value: "respond_provider_update_prompt".to_string(),
            args: Some(serde_json::json!({ "promptId": prompt_id, "accept": accept })),
        };
        let actions = vec![respond("Update", true), respond("Not now", false)];
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
            PushDispatch::inert(),
            SurfaceDispatch::silent(),
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
        severity: Option<&str>,
    ) -> String {
        create_notification_with_delivery(
            NotificationAdmission::Diagnostic,
            &self.notif,
            |_, _| {},
            title,
            body,
            severity,
            None,
            None,
            NotificationOrigin::default(),
            |_| None,
            PushDispatch::inert(),
            SurfaceDispatch::silent(),
            |_| {},
        )
    }

    /// Explicitly requested notification (CLI/REST/frontend surfaces).
    #[allow(clippy::too_many_arguments)]
    pub fn create_notification_ext(
        &self,
        title: String,
        body: Option<String>,
        severity: Option<&str>,
        actions: Option<Vec<NotificationAction>>,
        sticky: Option<bool>,
        origin: NotificationOrigin,
        intent: PushIntent,
    ) -> String {
        // The declared level outlives the push policy: an install with no
        // Telegram audience still has a desktop gate to satisfy.
        let declared = intent.level;
        let (intent, per_scope_limit, audience) = self.resolve_push_policy(intent);
        let surfaces = self.resolve_surface_dispatch(declared);
        create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &self.notif,
            |title, body| {
                crate::api::os_notify::_send_os_notification(&self.app_handle, title, body)
            },
            title,
            body,
            severity,
            actions,
            sticky,
            origin,
            |candidate| self.resolve_tab_provenance(candidate),
            PushDispatch {
                intent,
                per_scope_limit,
                deliver: move |envelope: PushEnvelope| {
                    self.deliver_telegram_push(&audience, &envelope)
                },
            },
            surfaces,
            |payload| self.notify_event("notification-created", payload),
        )
    }

    /// Record the user's choice. The command carries the record and the chosen
    /// label only — the tab it answers to was captured when the question was
    /// sent, so nothing is resolved here.
    pub fn answer_notification(
        &self,
        id: &str,
        label: &str,
    ) -> Result<serde_json::Value, String> {
        // The guard is dropped before the event fans out: nothing downstream of
        // an emit may re-enter the store while it is still held.
        let admission = self.notif.lock().record_answer(id, label);
        match admission {
            // Carrying the answer back is an ordered consequence of recording
            // it: the send happens before this call returns.
            AnswerAdmission::Recorded(chosen) => Ok(self.settle_answer(id, chosen)),
            // A second tap is not an error to the user: the standing answer is
            // what the question holds, and it is unchanged. A retry may still
            // finish a delivery the first attempt could not.
            AnswerAdmission::AlreadyAnswered(standing) => Ok(self.settle_answer(id, standing)),
            AnswerAdmission::Undeliverable => {
                Err("The tab that asked this question is gone".to_string())
            }
            AnswerAdmission::NotOffered => {
                Err("That choice was not offered by this notification".to_string())
            }
        }
    }

    /// Carry the standing answer to the tab that asked, as a caller of the
    /// existing envelope path, and report what actually happened. A refused
    /// send leaves the record answered-but-undelivered, never delivered.
    fn settle_answer(&self, id: &str, standing: String) -> serde_json::Value {
        let outcome = answer_delivery::deliver(&self.notif, id, |to_tab, envelope, body| {
            crate::service::messenger::Messenger::deliver(self, to_tab, envelope, body).map(|_| ())
        });
        if let Some(detail) = outcome.failure() {
            log::warn!("[notify-answer] {id} recorded but not delivered: {detail}");
        }
        let delivered = outcome.reached_tab();
        self.notify_event(
            "notification-answered",
            serde_json::json!({ "id": id, "answered": standing, "delivered": delivered }),
        );
        match outcome {
            AnswerDeliveryOutcome::Failed(detail) => serde_json::json!({
                "answered": standing,
                "delivered": false,
                "deliveryError": detail,
            }),
            _ => serde_json::json!({ "answered": standing, "delivered": delivered }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::store::NotificationStore;
    use codeterm_types::config::schema::NotificationConfig;
    use parking_lot::Mutex;
    use std::sync::Arc;

    /// A caller that named a level, judged against the shipped policy.
    fn declared(level: NotifyLevel) -> SurfaceDispatch {
        SurfaceDispatch {
            declared: Some(level),
            bell: BellPolicy::from_config(&NotificationConfig::default()),
            desktop: DesktopPolicy::from_config(&NotificationConfig::default()),
        }
    }

    /// A caller that named none — refused by the gate, not by an empty policy.
    fn undeclared() -> SurfaceDispatch {
        SurfaceDispatch {
            declared: None,
            bell: BellPolicy::from_config(&NotificationConfig::default()),
            desktop: DesktopPolicy::from_config(&NotificationConfig::default()),
        }
    }

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
            PushDispatch::inert(),
            declared(NotifyLevel::Verbose),
            |payload| emitted.lock().push(payload),
        );

        let guard = store.lock();
        let listed = guard.list(false);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id, "returned id names the stored record");
        assert_eq!(listed[0].title, "Build finished");
        assert_eq!(listed[0].body.as_deref(), Some("release artifacts ready"));
        assert_eq!(listed[0].severity, NotificationSeverity::Warning);
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
            PushDispatch::inert(),
            declared(NotifyLevel::ActionNeeded),
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
                PushDispatch::inert(),
                SurfaceDispatch::silent(),
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
                PushDispatch::inert(),
                SurfaceDispatch::silent(),
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
                PushDispatch::inert(),
                SurfaceDispatch::silent(),
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
                PushDispatch::inert(),
                SurfaceDispatch::silent(),
                |_| {},
            );

            let guard = store.lock();
            let listed = guard.list(false);
            assert_eq!(listed[0].tab_id, None);
            assert_eq!(listed[0].workspace_id, None);
            assert_eq!(listed[0].peer_name.as_deref(), Some("BLACKSTAR"));
        }
    }

    mod telegram_render {
        use super::*;
        use crate::notification::push::MAX_BODY_CHARS;

        fn live_tab(candidate: &str) -> Option<NotificationProvenance> {
            (candidate == "tab_worker").then(|| NotificationProvenance {
                tab_id: Some("tab_worker".into()),
                tab_title: Some("Worker lane".into()),
                workspace_id: Some("ws_lanes".into()),
                peer_name: None,
            })
        }

        fn dispatch<F>(level: NotifyLevel, deliver: F) -> PushDispatch<F>
        where
            F: FnOnce(PushEnvelope) -> Result<(), String>,
        {
            PushDispatch {
                intent: PushIntent {
                    level: Some(level),
                    event_key: Some("release-42".into()),
                },
                per_scope_limit: 10,
                deliver,
            }
        }

        #[test]
        fn telegram_send_failure_does_not_change_the_returned_id() {
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let emitted = Mutex::new(Vec::new());
            let attempted = Mutex::new(Vec::new());

            // Bot blocked / chat unreachable: the sink refuses the envelope.
            let id = create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Release cut".into(),
                Some("artifacts uploaded".into()),
                Some("info"),
                None,
                None,
                NotificationOrigin::claimed(Some("tab_worker".into())),
                live_tab,
                dispatch(NotifyLevel::Verbose, |envelope: PushEnvelope| {
                    attempted.lock().push(envelope.text.clone());
                    Err("chat unreachable".to_string())
                }),
                declared(NotifyLevel::Verbose),
                |payload| emitted.lock().push(payload),
            );

            assert_eq!(attempted.lock().len(), 1, "the sink was actually invoked");
            let guard = store.lock();
            let listed = guard.list(false);
            assert_eq!(listed.len(), 1, "durable history survives a refused push");
            assert_eq!(
                listed[0].id, id,
                "the caller's id names the stored record whether or not the push landed"
            );
            assert_eq!(
                listed[0].push_failed,
                Some(true),
                "the failure is recorded on the record, not returned"
            );
            drop(guard);

            let events = emitted.lock();
            assert_eq!(events.len(), 1, "the local routes still fire");
            assert_eq!(events[0]["id"], id);
        }

        #[test]
        fn a_delivered_push_carries_capped_text_and_leaves_the_record_clean() {
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let oversized = "b".repeat(MAX_BODY_CHARS + 200);
            let sent = Mutex::new(Vec::new());

            let id = create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Approval required".into(),
                Some(oversized.clone()),
                Some("warning"),
                None,
                None,
                NotificationOrigin::claimed(Some("tab_worker".into())),
                live_tab,
                dispatch(NotifyLevel::ActionNeeded, |envelope: PushEnvelope| {
                    sent.lock().push(envelope.text.clone());
                    Ok(())
                }),
                declared(NotifyLevel::ActionNeeded),
                |_| {},
            );

            let delivered = sent.lock();
            assert_eq!(delivered.len(), 1);
            assert_eq!(
                delivered[0].matches('b').count(),
                MAX_BODY_CHARS,
                "the agent body is truncated to the cap constant, not rejected"
            );
            assert!(delivered[0].contains("via Worker lane"));
            assert!(!delivered[0].contains("tab_worker"));
            drop(delivered);

            assert_eq!(store.lock().list(false)[0].push_failed, None);
            assert_eq!(store.lock().list(false)[0].id, id);
        }

        #[test]
        fn an_unlevelled_notification_never_reaches_the_push_sink() {
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let sent = Mutex::new(Vec::new());

            create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Still working".into(),
                Some("step 4 of 9".into()),
                Some("info"),
                None,
                None,
                NotificationOrigin::claimed(Some("tab_worker".into())),
                live_tab,
                PushDispatch {
                    intent: PushIntent::default(),
                    per_scope_limit: 10,
                    deliver: |envelope: PushEnvelope| {
                        sent.lock().push(envelope.text.clone());
                        Ok(())
                    },
                },
                undeclared(),
                |_| {},
            );

            assert!(
                sent.lock().is_empty(),
                "routine progress has no level, so it cannot reach the channel"
            );
            assert_eq!(store.lock().list(false).len(), 1);
        }
    }

    /// Both sinks a first arrival reaches: the OS toast and the bell event.
    /// The gate is open here on purpose — dedupe decides whether a message is
    /// new, and that question is only interesting once it is allowed at all.
    fn deliver(
        store: &NotifState,
        title: &str,
        body: Option<&str>,
        native: &Mutex<Vec<String>>,
        emitted: &Mutex<Vec<serde_json::Value>>,
    ) -> String {
        create_notification_with_delivery(
            NotificationAdmission::Explicit,
            store,
            |title, _| native.lock().push(title.to_string()),
            title.to_string(),
            body.map(str::to_string),
            Some("info"),
            None,
            None,
            NotificationOrigin::default(),
            |_| None,
            PushDispatch::inert(),
            declared(NotifyLevel::ActionNeeded),
            |payload| emitted.lock().push(payload),
        )
    }

    #[test]
    fn notify_dedupe_same_message_reaches_the_sinks_once() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let native = Mutex::new(Vec::new());
        let emitted = Mutex::new(Vec::new());

        let title = "agent needs input";
        let body = Some("approve the destructive step?");
        let first = deliver(&store, title, body, &native, &emitted);
        let second = deliver(&store, title, body, &native, &emitted);

        assert_eq!(second, first, "the duplicate names the record that exists");
        assert_eq!(native.lock().len(), 1, "one native toast, not two");
        assert_eq!(emitted.lock().len(), 1, "one bell event, not two");
        assert_eq!(store.lock().list(false).len(), 1, "one record, not two");
    }

    #[test]
    fn notify_dedupe_distinct_messages_both_reach_the_sinks() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let native = Mutex::new(Vec::new());
        let emitted = Mutex::new(Vec::new());

        let first = deliver(&store, "build failed", Some("linker error"), &native, &emitted);
        let other_body = deliver(&store, "build failed", Some("test error"), &native, &emitted);
        let other_title = deliver(&store, "build passed", Some("linker error"), &native, &emitted);

        assert_ne!(first, other_body);
        assert_ne!(first, other_title);
        assert_eq!(native.lock().len(), 3);
        assert_eq!(emitted.lock().len(), 3);
        assert_eq!(store.lock().list(false).len(), 3);
    }

    #[test]
    fn notify_dedupe_an_acknowledged_message_may_arrive_again() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let native = Mutex::new(Vec::new());
        let emitted = Mutex::new(Vec::new());

        let first = deliver(&store, "codex available", None, &native, &emitted);
        assert!(store.lock().mark_read(&first));
        let again = deliver(&store, "codex available", None, &native, &emitted);

        assert_ne!(
            again, first,
            "a recurrence after the user acknowledged it is a new event"
        );
        assert_eq!(native.lock().len(), 2);
        assert_eq!(store.lock().unread_count(), 1);
    }

    #[test]
    fn notify_dedupe_standing_condition_survives_a_cleared_inbox() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let scope = push::tab_scope("tab_a");
        let condition = crate::notification::dedupe::provider_update_condition("codex");

        assert_eq!(
            store.lock().observe_condition_present(&scope, &condition),
            crate::notification::dedupe::ConditionTransition::Announce
        );
        store.lock().clear(None);
        assert_eq!(
            store.lock().observe_condition_present(&scope, &condition),
            crate::notification::dedupe::ConditionTransition::AlreadyAnnounced,
            "wiping history does not make a still-true condition news again"
        );

        store.lock().observe_condition_absent(&scope, &condition);
        assert_eq!(
            store.lock().observe_condition_present(&scope, &condition),
            crate::notification::dedupe::ConditionTransition::Announce,
            "only the condition resolving re-arms the announcement"
        );
    }

    /// `level` and `severity` are independent axes on the wire — these
    /// exercise the payload, which is what the surfaces actually read.
    mod notification_level_field {
        use super::*;

        #[test]
        fn notification_level_field_reaches_stored_record_and_emitted_payload() {
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let emitted = Mutex::new(Vec::new());

            let id = create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Which branch?".into(),
                None,
                Some("info"),
                None,
                None,
                NotificationOrigin::default(),
                |_| None,
                PushDispatch::inert(),
                declared(NotifyLevel::ActionNeeded),
                |payload| emitted.lock().push(payload),
            );

            assert_eq!(
                store.lock().level(&id),
                Some(NotifyLevel::ActionNeeded),
                "the declared level is written onto the stored record"
            );
            let events = emitted.lock();
            assert_eq!(events.len(), 1);
            assert_eq!(
                events[0]["level"], "action_needed",
                "the surfaces read the payload, so the level must be on it"
            );
        }

        #[test]
        fn notification_level_field_absent_stays_absent_and_still_notifies() {
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let emitted = Mutex::new(Vec::new());

            let id = create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Still working".into(),
                Some("step 4 of 9".into()),
                Some("info"),
                None,
                None,
                NotificationOrigin::default(),
                |_| None,
                PushDispatch::inert(),
                undeclared(),
                |payload| emitted.lock().push(payload),
            );

            assert_eq!(
                store.lock().level(&id),
                None,
                "naming no level is a legal, distinct state, not a lost one"
            );
            let guard = store.lock();
            let listed = guard.list(false);
            assert_eq!(listed.len(), 1, "the record is still kept");
            assert!(!listed[0].read, "it still counts as unread");
            drop(guard);

            let events = emitted.lock();
            assert_eq!(events.len(), 1, "the bell still rings");
            assert!(
                events[0].get("level").is_none(),
                "an absent level must not appear on the payload at all"
            );
        }

        #[test]
        fn notification_level_field_independent_of_severity_survives_no_push_audience() {
            // `PushDispatch::inert()` mirrors `resolve_push_policy` clearing
            // intent with no audience — `surfaces.declared` must still land.
            let store = Arc::new(Mutex::new(NotificationStore::new(10)));
            let emitted = Mutex::new(Vec::new());

            create_notification_with_delivery(
                NotificationAdmission::Explicit,
                &store,
                |_, _| {},
                "Which branch?".into(),
                None,
                Some("info"),
                None,
                None,
                NotificationOrigin::default(),
                |_| None,
                PushDispatch::inert(),
                declared(NotifyLevel::ActionNeeded),
                |payload| emitted.lock().push(payload),
            );

            let events = emitted.lock();
            assert_eq!(events[0]["severity"], "Info");
            assert_eq!(
                events[0]["level"], "action_needed",
                "severity being the info default must not drag the level down with it"
            );
        }
    }
}
