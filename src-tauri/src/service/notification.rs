//! Notification creation with OS toast support.

use codeterm_types::notification::{
    adopt_caller_actions, CallerAction, DaemonCommand, NotificationAction,
};

use super::ServiceContext;
use crate::notification::{
    answer::AnswerAdmission,
    answer_delivery::{self, AnswerDeliveryOutcome},
    authority::{self, SurfaceOffer},
    channel_ask::{self, PushMessage},
    dedupe::DeliveryAdmission,
    desktop::{self, DesktopPolicy},
    push::{self, NotifyLevel},
    reach::{self, Unseen},
    store::{Ask, DedupAdmission, NotificationProvenance, NotificationSeverity, PushRequest},
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
    pub notification_id: String,
    /// The rendered text and the offered options, each carrying the opaque
    /// token that names it. A channel draws them; the tapped token comes back
    /// through the one answer path the in-app card already uses.
    pub message: PushMessage,
}

impl PushEnvelope {
    pub fn text(&self) -> &str {
        &self.message.text
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
/// The panel is not represented here. It is the complete log and takes every
/// record unconditionally, so there is no policy for it to consult.
pub struct SurfaceDispatch {
    pub declared: Option<NotifyLevel>,
    pub desktop: DesktopPolicy,
}

impl SurfaceDispatch {
    /// Routes that raise no toast at all: internal diagnostics and the in-app
    /// provider-update prompt. They still reach the panel, which is where they
    /// have always landed — silence is about interrupting, not about recording.
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
pub(crate) enum NotificationAdmission {
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
pub(crate) fn create_notification_with_delivery(
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
    if matches!(admission, NotificationAdmission::Diagnostic) {
        return uuid::Uuid::new_v4().to_string();
    }
    let actions = ask.actions.clone();
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
            // record and reaches neither the native sink nor the panel again.
            // Push routing may clear its own intent (nothing routed anywhere);
            // the caller's declared level still stands and is what gets stored.
            None => match store.lock().add_deduped(
                title.clone(),
                body.clone(),
                parsed_severity,
                provenance.clone(),
                ask,
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
                    ask,
                });
                // A duplicate reuses an open record: nothing new to toast or emit.
                if !admitted.opened_or_changed_record() {
                    return admitted.id().to_string();
                }
                if admitted.should_push() {
                    let envelope = PushEnvelope {
                        level,
                        message: PushMessage {
                            text: push::render_push(level, &title, body.as_deref(), &provenance),
                            // Read off the record: the position a token names
                            // is the position that record will resolve it at.
                            choices: store.lock().offered_choices_for_channel(admitted.id()),
                        },
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
    // The panel is the complete log, so every record reaches it: this event is
    // the only live path to it, and gating it would leave the panel silently
    // lagging behind a store that already holds the record. `show_bell` is a
    // presentation choice about the indicator and never reaches this gate.
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
        let respond = |label: &str, accept: bool| NotificationAction::Invoke {
            label: label.to_string(),
            command: DaemonCommand::RespondProviderUpdatePrompt,
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
            Ask::offering(Some(actions)),
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
            Ask::default(),
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
        actions: Option<Vec<CallerAction>>,
        sticky: Option<bool>,
        origin: NotificationOrigin,
        intent: PushIntent,
        offer: Option<SurfaceOffer>,
    ) -> String {
        self.create_notification_ext_reported(
            title, body, severity, actions, sticky, origin, intent, offer,
        )
        .0
    }

    /// The same explicit notification, plus the honest answer to "will anyone
    /// see this". The record is opened either way — the report is information
    /// for the caller, never a refusal.
    #[allow(clippy::too_many_arguments)]
    pub fn create_notification_ext_reported(
        &self,
        title: String,
        body: Option<String>,
        severity: Option<&str>,
        actions: Option<Vec<CallerAction>>,
        sticky: Option<bool>,
        origin: NotificationOrigin,
        intent: PushIntent,
        offer: Option<SurfaceOffer>,
    ) -> (String, Option<Unseen>) {
        // The provenance seam: a caller-supplied set becomes stored actions
        // here, through a conversion with no invoke arm to reach.
        let actions = actions.map(adopt_caller_actions);
        let ask = Ask { actions, offer };
        // The declared level outlives the push policy: an install whose
        // channels resolve to nobody still has a desktop gate to satisfy.
        let declared = intent.level;
        let super::notification_push::PushPlan {
            intent,
            per_scope_limit,
            targets,
            refusals,
        } = self.resolve_push_plan(intent);
        let surfaces = self.resolve_surface_dispatch(declared);
        // Judged before the plan is moved into the sink, off the same targets
        // the sink will use — so the report cannot disagree with the delivery.
        let unseen = reach::unseen(reach::Reach {
            show_bell: self.bell_visible(),
            declared,
            desktop: &surfaces.desktop,
            push: reach::PushReach {
                carrying: targets.len(),
                refused: refusals
                    .iter()
                    .map(|refusal| refusal.service.clone())
                    .collect(),
            },
        });
        let id = create_notification_with_delivery(
            NotificationAdmission::Explicit,
            &self.notif,
            |title, body| {
                crate::api::os_notify::_send_os_notification(&self.app_handle, title, body)
            },
            title,
            body,
            severity,
            ask,
            sticky,
            origin,
            |candidate| self.resolve_tab_provenance(candidate),
            PushDispatch {
                intent,
                per_scope_limit,
                deliver: move |envelope: PushEnvelope| {
                    super::notification_push::deliver_plan(&targets, &refusals, self, &envelope)
                },
            },
            surfaces,
            |payload| self.notify_event("notification-created", payload),
        );
        (id, unseen)
    }

    /// Record the user's choice. The command carries the record and the chosen
    /// label only — the tab it answers to was captured when the question was
    /// sent, so nothing is resolved here.
    pub fn answer_notification(&self, id: &str, label: &str) -> Result<serde_json::Value, String> {
        // The guard is dropped before the event fans out: nothing downstream of
        // an emit may re-enter the store while it is still held.
        let admission = self.notif.lock().record_answer(id, label);
        // THE AUTHORITY RULE, at the one place a surface may widen: the proof
        // exists only for a first-and-only recording, so an offer applies on
        // the tap that answered it and on nothing else.
        if let Some(proof) = authority::tap_proof(&admission) {
            self.apply_surface_offer(id, label, proof);
        }
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

    /// An answer arrived on a push channel. The token names a position on one
    /// record; the label is resolved off that record and then walks the very
    /// same answer path an in-app tap walks, so the tapped label returns to the
    /// tab that asked and no second answer route exists. `false` means this was
    /// ordinary channel traffic and nothing was claimed.
    pub fn claim_channel_answer(&self, callback_data: &str) -> bool {
        let Some(tapped) = channel_ask::decode(callback_data) else {
            return false;
        };
        // The guard is taken and dropped before the answer path runs: that path
        // takes the same store lock, and parking_lot is not reentrant.
        let resolved = {
            self.notif
                .lock()
                .offered_choice(&tapped.notification_id, tapped.index)
        };
        let Some(label) = resolved else {
            log::warn!(
                "[notify-answer] a channel tap named no offered choice on {}",
                tapped.notification_id
            );
            return true;
        };
        if let Err(reason) = self.answer_notification(&tapped.notification_id, &label) {
            log::warn!(
                "[notify-answer] a channel tap on {} was refused: {reason}",
                tapped.notification_id
            );
        }
        true
    }

    /// Apply whatever this record offered, and only when the tapped label is
    /// the one it named. An untapped or declined offer never reaches here, and
    /// what changes is the one field `authority::apply` writes.
    fn apply_surface_offer(&self, id: &str, label: &str, proof: authority::TapProof) {
        // The level is read off the record the tap answered, never passed in
        // beside it: what the tap grants is the kind of notification the
        // question itself demonstrated, and only the record knows which that was.
        let standing = {
            let store = self.notif.lock();
            store.surface_offer(id).zip(store.level(id))
        };
        let Some((offer, level)) = standing else {
            return;
        };
        if !offer.accepted_by(label) {
            return;
        }
        let mut cfg = crate::config::load_config();
        authority::apply(proof, &mut cfg.notifications, &offer.target, level);
        if let Err(reason) = crate::config::save_config_pub(&cfg) {
            log::error!("[notify-offer] accepted offer on {id} could not be saved: {reason}");
            return;
        }
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "config-changed",
            serde_json::to_value(&cfg).unwrap_or_default(),
        );
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
mod tests;
