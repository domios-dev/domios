//! Notification creation with OS toast support.

use crate::notification::{
    channel_ask::{ChannelChoice, PushMessage},
    dedupe::DeliveryAdmission,
    deep_link::NotificationTarget,
    desktop::{self, DesktopPolicy},
    push::{self, NotifyLevel},
    store::{Ask, NotificationProvenance, NotificationSeverity, PushRequest},
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
///
/// The text is deliberately not rendered here: a channel's length bound is the
/// transport's, so the material travels unrendered and each channel renders it
/// at its own ceiling.
pub struct PushEnvelope {
    pub content: push::PushContent,
    /// The offered options, each carrying the opaque token that names it. A
    /// channel draws them; the tapped token comes back through the one answer
    /// path the in-app card already uses. Same on every channel.
    pub choices: Vec<ChannelChoice>,
    /// The record this renders, echoed by a channel and never read by one.
    pub record: String,
}

impl PushEnvelope {
    /// This push as one channel will carry it, bounded by that channel.
    pub fn message_for(&self, bounds: push::PushBounds) -> PushMessage {
        PushMessage {
            text: self.content.render(bounds),
            choices: self.choices.clone(),
            record: self.record.clone(),
            origin_tab: self.content.provenance.tab_id.clone(),
        }
    }
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

/// The in-app leg of one notification: what the caller declared, and the user's
/// standing policy for the one surface that can refuse. Deliberately not folded
/// into `PushDispatch` — the push opt-in clears its own intent when nobody is
/// subscribed, while the declared level still governs the toast.
///
/// The panel receives every admitted agent record.
pub struct SurfaceDispatch {
    pub declared: Option<NotifyLevel>,
    pub desktop: DesktopPolicy,
}

impl SurfaceDispatch {
    /// Disables desktop delivery without changing admission.
    pub fn silent() -> Self {
        Self {
            declared: None,
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
    /// The chat message this record jumps to. Navigation only, never provenance.
    pub target: NotificationTarget,
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

pub mod admission;
use admission::{destination, NoticeDestination, NoticeSource};

#[derive(Debug)]
pub enum NotificationAdmission {
    Diagnostic,
    /// Admitted through the agent notify-send boundary.
    Explicit,
}

fn notification_severity(severity: Option<&str>) -> NotificationSeverity {
    match severity {
        Some("warning") | Some("Warning") => NotificationSeverity::Warning,
        Some("error") | Some("Error") => NotificationSeverity::Error,
        _ => NotificationSeverity::Info,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_notification_with_delivery(
    admission: NotificationAdmission,
    store: &NotifState,
    deliver_native: impl FnOnce(&str, Option<&str>),
    title: String,
    body: Option<String>,
    severity: Option<&str>,
    ask: Ask,
    sticky: Option<bool>,
    origin: NotificationOrigin,
    resolve_tab: impl FnOnce(&str) -> Option<NotificationProvenance>,
    push: PushDispatch<impl FnOnce(PushEnvelope) -> Result<(), String>>,
    surfaces: SurfaceDispatch,
    emit: impl FnOnce(serde_json::Value),
) -> String {
    let source = match admission {
        NotificationAdmission::Diagnostic => NoticeSource::System,
        NotificationAdmission::Explicit => NoticeSource::AgentNotifySend,
    };
    if destination(source, true) == NoticeDestination::Log {
        let detail = body.as_deref().unwrap_or_default();
        match notification_severity(severity) {
            NotificationSeverity::Error => log::error!("[system-notice] {title}: {detail}"),
            NotificationSeverity::Warning => log::warn!("[system-notice] {title}: {detail}"),
            NotificationSeverity::Info => log::info!("[system-notice] {title}: {detail}"),
        }
        return uuid::Uuid::new_v4().to_string();
    }
    let actions = ask.actions.clone();
    let provenance = resolve_origin(&origin, resolve_tab);
    // A remote peer never names a local tab, so it cannot name one to jump to either.
    let target = if origin.peer_name.is_some() {
        NotificationTarget::default()
    } else {
        origin.target.clone()
    };
    let explicit = matches!(admission, NotificationAdmission::Explicit);
    let parsed_severity = notification_severity(severity);
    let severity_str = match &parsed_severity {
        NotificationSeverity::Warning => "Warning",
        NotificationSeverity::Error => "Error",
        _ => "Info",
    };
    let id = match admission {
        NotificationAdmission::Diagnostic => unreachable!(),
        NotificationAdmission::Explicit => match push.intent.level {
            // An identical live message is the same message: it keeps its record and reaches neither the native sink nor the panel again.
            None => {
                let mut store = store.lock();
                let keyed_question = ask.condition.is_none()
                    && crate::notification::answer::slot_for(
                        ask.actions.as_deref(),
                        provenance.tab_id.as_deref(),
                    )
                    .is_some();
                let admitted = if let Some(event_key) =
                    push.intent.event_key.as_deref().filter(|_| keyed_question)
                {
                    let scope = push::scope_key(&provenance);
                    let event_key = push::event_key(Some(event_key), &title);
                    store.supersede_question(&scope, &event_key);
                    let id = store.add_ext(
                        title.clone(),
                        body.clone(),
                        parsed_severity,
                        provenance.clone(),
                        ask,
                        sticky,
                        surfaces.declared,
                    );
                    store.remember_question(scope, event_key, &id);
                    DeliveryAdmission::Deliver(id)
                } else {
                    store.add_deduped(
                        title.clone(),
                        body.clone(),
                        parsed_severity,
                        provenance.clone(),
                        ask,
                        sticky,
                        surfaces.declared,
                    )
                };
                match admitted {
                    DeliveryAdmission::Deliver(id) => id,
                    DeliveryAdmission::Duplicate(id) => return id,
                }
            }
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
                    ask,
                });
                // A duplicate reuses an open record: nothing new to toast or emit.
                if !admitted.opened_or_changed_record() {
                    return admitted.id().to_string();
                }
                if admitted.should_push() {
                    let envelope = PushEnvelope {
                        content: push::PushContent {
                            level,
                            title: title.clone(),
                            body: body.clone(),
                            provenance: provenance.clone(),
                        },
                        // Read off the record: the position a token names is
                        // the position that record will resolve it at.
                        choices: store.lock().offered_choices_for_channel(admitted.id()),
                        record: admitted.id().to_string(),
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
    };

    if explicit {
        store.lock().set_target(&id, target);
    }
    let stored = store.lock().notification(&id).cloned();

    // The answer slot the store just built, so every surface renders the same
    // capture rather than deriving its own.
    let answer = store.lock().answer_slot(&id).cloned();
    // Read back off the record rather than reusing `surfaces.declared`: the
    // payload must always agree with what was actually stored.
    let level = store.lock().level(&id);
    let mut payload = serde_json::json!({
        "id": id,
        "title": stored.as_ref().map(|record| record.title.clone()).unwrap_or(title),
        "body": stored.as_ref().and_then(|record| record.body.clone()).or(body),
        "severity": severity_str,
        "tab_id": provenance.tab_id.clone(),
        "workspace_id": provenance.workspace_id.clone(),
        "tab_title": provenance.tab_title.clone(),
        "peer_name": provenance.peer_name.clone(),
    });
    if let Some(record) = stored.as_ref() {
        if let Some(tab_id) = &record.target_tab_id {
            payload["target_tab_id"] = serde_json::json!(tab_id);
        }
        if let Some(message_id) = &record.chat_message_id {
            payload["chat_message_id"] = serde_json::json!(message_id);
        }
    }
    let emitted_actions = stored
        .as_ref()
        .and_then(|record| record.actions.clone())
        .or(actions);
    if let Some(ref acts) = emitted_actions {
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
    // go. Unlevelled keeps its record and its panel arrival, and stops.
    if let Some(pass) = desktop::admit(surfaces.declared, &surfaces.desktop).pass() {
        log::debug!(
            "[notify-gate] desktop toast admitted as {}",
            pass.level().as_str()
        );
        deliver_native(
            payload["title"].as_str().unwrap_or_default(),
            payload["body"].as_str(),
        );
    }
    // The panel must receive every record; `show_bell` only controls indicator presentation and must not gate delivery.
    emit(payload);
    id
}
