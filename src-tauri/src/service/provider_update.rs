use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

use crate::agent::provider::{ProviderSelfUpdateCapability, ProviderUpdateCompletionSignal};
use crate::agent::status::TabStatus;
use crate::multiplexer::types::{PendingDeliveryState, TabAudience};

use super::ServiceContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentPrincipal {
    LocalInteractive,
    Unauthorized(DeclineReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclineReason {
    Automation,
    MeshDispatched,
    Unsupported,
    StalePrompt,
    UserDeclined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedConsent {
    pub prompt_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkInFlight {
    pub delivery_state: Option<PendingDeliveryState>,
    pub delivery_attempt_active: bool,
    pub provider_turn_active: bool,
}

impl WorkInFlight {
    fn restart_safe(self) -> bool {
        !self.delivery_attempt_active
            && !self.provider_turn_active
            && self
                .delivery_state
                .is_none_or(PendingDeliveryState::is_accepted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    AwaitConsent,
    Decline(DeclineReason),
    Defer,
    Install { restart_required: bool },
}

pub fn decide(
    prompt_id: &str,
    principal: ConsentPrincipal,
    capability: Option<ProviderSelfUpdateCapability>,
    consent: Option<&RecordedConsent>,
    work: WorkInFlight,
) -> PolicyDecision {
    let Some(capability) = capability.filter(|capability| capability.in_session) else {
        return PolicyDecision::Decline(DeclineReason::Unsupported);
    };
    if let ConsentPrincipal::Unauthorized(reason) = principal {
        return PolicyDecision::Decline(reason);
    }
    match consent {
        None => return PolicyDecision::AwaitConsent,
        Some(consent) if consent.prompt_id != prompt_id => {
            return PolicyDecision::Decline(DeclineReason::StalePrompt)
        }
        Some(_) => {}
    }
    if !work.restart_safe() {
        return PolicyDecision::Defer;
    }
    PolicyDecision::Install {
        restart_required: capability.restart_required,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUpdateStatus {
    AwaitingConsent,
    Declined,
    Deferred,
    Installing,
    Completed,
    Restarted,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderUpdatePromptView {
    pub prompt_id: String,
    pub tab_id: String,
    pub provider_name: String,
    pub restart_required: bool,
    pub status: ProviderUpdateStatus,
    pub message: String,
}

#[derive(Clone)]
struct PendingProviderUpdate {
    view: ProviderUpdatePromptView,
    provider_id: String,
    pty_id: String,
    capability: Option<ProviderSelfUpdateCapability>,
    install_writes: Vec<Vec<u8>>,
    decline_writes: Vec<Vec<u8>>,
    consent: Option<RecordedConsent>,
    phase: ProviderUpdatePhase,
    waiter_started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderUpdatePhase {
    AwaitingConsent,
    Deferred,
    Installing,
    Completing,
    Terminal,
}

enum ClaimedResponse {
    Accept(PendingProviderUpdate),
    Decline(PendingProviderUpdate, DeclineReason),
}

#[cfg(test)]
#[derive(Debug, Clone)]
enum ProviderUpdateTestEffect {
    Emitted(ProviderUpdatePromptView),
    Wrote { pty_id: String, bytes: Vec<u8> },
    Restarted { tab_id: String },
}

#[derive(Clone, Default)]
pub struct ProviderUpdateRegistry {
    entries: Arc<Mutex<HashMap<String, PendingProviderUpdate>>>,
    #[cfg(test)]
    probe: Arc<Mutex<Option<std::sync::mpsc::Sender<ProviderUpdateTestEffect>>>>,
}

impl ProviderUpdateRegistry {
    pub fn list(&self) -> Vec<ProviderUpdatePromptView> {
        self.entries
            .lock()
            .values()
            .map(|entry| entry.view.clone())
            .collect()
    }

    fn insert(&self, entry: PendingProviderUpdate) {
        self.entries
            .lock()
            .insert(entry.view.prompt_id.clone(), entry);
    }

    fn get(&self, prompt_id: &str) -> Option<PendingProviderUpdate> {
        self.entries.lock().get(prompt_id).cloned()
    }

    fn claim_response(
        &self,
        prompt_id: &str,
        accept: bool,
        principal: ConsentPrincipal,
    ) -> Result<ClaimedResponse, String> {
        let mut entries = self.entries.lock();
        let entry = entries
            .get_mut(prompt_id)
            .ok_or_else(|| "Provider update prompt is no longer active".to_string())?;
        if entry.phase != ProviderUpdatePhase::AwaitingConsent {
            return Err("Provider update prompt authority was already consumed".to_string());
        }
        let decline_reason = match principal {
            ConsentPrincipal::Unauthorized(reason) => Some(reason),
            ConsentPrincipal::LocalInteractive if !accept => Some(DeclineReason::UserDeclined),
            ConsentPrincipal::LocalInteractive => None,
        };
        if let Some(reason) = decline_reason {
            entry.phase = ProviderUpdatePhase::Terminal;
            entry.view.status = ProviderUpdateStatus::Declined;
            entry.view.message = format!("Update declined: {}.", decline_reason_text(reason));
            return Ok(ClaimedResponse::Decline(entry.clone(), reason));
        }
        entry.phase = ProviderUpdatePhase::Deferred;
        entry.consent = Some(RecordedConsent {
            prompt_id: prompt_id.to_string(),
        });
        Ok(ClaimedResponse::Accept(entry.clone()))
    }

    fn remove(&self, prompt_id: &str) -> Option<PendingProviderUpdate> {
        self.entries.lock().remove(prompt_id)
    }

    fn defer(&self, prompt_id: &str, message: String) -> Option<PendingProviderUpdate> {
        let mut entries = self.entries.lock();
        let entry = entries.get_mut(prompt_id)?;
        if entry.phase != ProviderUpdatePhase::Deferred {
            return None;
        }
        entry.view.status = ProviderUpdateStatus::Deferred;
        entry.view.message = message;
        Some(entry.clone())
    }

    fn claim_install(&self, prompt_id: &str) -> Option<PendingProviderUpdate> {
        let mut entries = self.entries.lock();
        let entry = entries.get_mut(prompt_id)?;
        if entry.phase != ProviderUpdatePhase::Deferred {
            return None;
        }
        entry.phase = ProviderUpdatePhase::Installing;
        entry.view.status = ProviderUpdateStatus::Installing;
        entry.view.message = "Installing the authorized provider update.".to_string();
        Some(entry.clone())
    }

    fn mark_waiter_started(&self, prompt_id: &str) -> bool {
        let mut entries = self.entries.lock();
        let Some(entry) = entries.get_mut(prompt_id) else {
            return false;
        };
        if entry.waiter_started || entry.phase != ProviderUpdatePhase::Deferred {
            return false;
        }
        entry.waiter_started = true;
        true
    }

    fn claim_completion(&self, tab_id: &str, pty_id: &str) -> Option<PendingProviderUpdate> {
        let mut entries = self.entries.lock();
        let entry = entries.values_mut().find(|entry| {
            entry.view.tab_id == tab_id
                && entry.pty_id == pty_id
                && entry.phase == ProviderUpdatePhase::Installing
                && entry
                    .capability
                    .and_then(|capability| capability.completion_signal)
                    == Some(ProviderUpdateCompletionSignal::ProviderClassification)
        })?;
        entry.phase = ProviderUpdatePhase::Completing;
        Some(entry.clone())
    }

    fn finish(
        &self,
        mut entry: PendingProviderUpdate,
        status: ProviderUpdateStatus,
        message: String,
    ) {
        entry.phase = ProviderUpdatePhase::Terminal;
        entry.view.status = status;
        entry.view.message = message;
        self.entries
            .lock()
            .insert(entry.view.prompt_id.clone(), entry);
    }

    #[cfg(test)]
    fn set_probe(&self, sender: std::sync::mpsc::Sender<ProviderUpdateTestEffect>) {
        *self.probe.lock() = Some(sender);
    }

    #[cfg(test)]
    fn record(&self, effect: ProviderUpdateTestEffect) {
        if let Some(sender) = self.probe.lock().as_ref() {
            let _ = sender.send(effect);
        }
    }
}

impl ServiceContext {
    pub(crate) fn handle_provider_update_prompt(
        &self,
        tab_id: &str,
        provider_id: &str,
        capability: Option<ProviderSelfUpdateCapability>,
        install_writes: Vec<Vec<u8>>,
        decline_writes: Vec<Vec<u8>>,
    ) {
        let Some((pty_id, principal, work)) = self.provider_update_context(tab_id) else {
            return;
        };
        let provider_name = self
            .agent_registry
            .with_provider(provider_id, |provider| provider.display_name().to_string())
            .unwrap_or_else(|| provider_id.to_string());
        let prompt_id = format!("update_{}", uuid::Uuid::new_v4().simple());
        let decision = decide(&prompt_id, principal, capability, None, work);
        let (status, message) = match decision {
            PolicyDecision::AwaitConsent => (
                ProviderUpdateStatus::AwaitingConsent,
                format!("{provider_name} is requesting permission to install an update."),
            ),
            PolicyDecision::Decline(reason) => (
                ProviderUpdateStatus::Declined,
                format!("Update declined: {}.", decline_reason_text(reason)),
            ),
            _ => return,
        };
        let entry = PendingProviderUpdate {
            view: ProviderUpdatePromptView {
                prompt_id: prompt_id.clone(),
                tab_id: tab_id.to_string(),
                provider_name,
                restart_required: capability.is_some_and(|value| value.restart_required),
                status,
                message,
            },
            provider_id: provider_id.to_string(),
            pty_id,
            capability,
            install_writes,
            decline_writes,
            consent: None,
            phase: if matches!(decision, PolicyDecision::AwaitConsent) {
                ProviderUpdatePhase::AwaitingConsent
            } else {
                ProviderUpdatePhase::Terminal
            },
            waiter_started: false,
        };
        self.provider_updates.insert(entry.clone());
        self.emit_provider_update(&entry.view);
        if matches!(decision, PolicyDecision::Decline(_)) {
            if let Err(error) = self.write_provider_update_keys(&entry, &entry.decline_writes) {
                self.fail_provider_update(entry.clone(), &error);
            }
            log::warn!(
                "[provider-update] declined tab={} provider={} reason={}",
                tab_id,
                provider_id,
                entry.view.message
            );
        }
    }

    pub fn list_provider_update_prompts(&self) -> Vec<ProviderUpdatePromptView> {
        self.provider_updates.list()
    }

    pub fn respond_provider_update_prompt(
        &self,
        prompt_id: &str,
        accept: bool,
    ) -> Result<(), String> {
        let entry = self
            .provider_updates
            .get(prompt_id)
            .ok_or_else(|| "Provider update prompt is no longer active".to_string())?;
        let mut receiver = self.events.subscribe();
        let claimed = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(&entry.view.tab_id)
                .ok_or_else(|| "Provider update tab is no longer available".to_string())?;
            self.provider_updates.claim_response(
                prompt_id,
                accept,
                consent_principal(tab.audience, tab.report_target_mesh_peer.is_some()),
            )?
        };
        match claimed {
            ClaimedResponse::Decline(entry, reason) => {
                self.emit_provider_update(&entry.view);
                self.write_provider_update_keys(&entry, &entry.decline_writes)?;
                log::info!(
                    "[provider-update] prompt consumed decision=decline prompt={} tab={} provider={} reason={:?}",
                    prompt_id,
                    entry.view.tab_id,
                    entry.provider_id,
                    reason
                );
            }
            ClaimedResponse::Accept(entry) => {
                log::info!(
                    "[provider-update] prompt consumed authority=local_interactive prompt={} tab={} provider={}",
                    prompt_id,
                    entry.view.tab_id,
                    entry.provider_id
                );
                if !self.try_execute_provider_update(prompt_id)? {
                    self.start_provider_update_waiter(prompt_id.to_string(), &mut receiver);
                }
            }
        }
        Ok(())
    }

    fn try_execute_provider_update(&self, prompt_id: &str) -> Result<bool, String> {
        let entry = match self.provider_updates.get(prompt_id) {
            Some(entry) => entry,
            None => return Ok(true),
        };
        let Some((current_pty, principal, work)) = self.provider_update_context(&entry.view.tab_id)
        else {
            self.fail_provider_update(entry, "Tab is no longer available");
            return Ok(true);
        };
        if current_pty != entry.pty_id {
            self.fail_provider_update(entry, "The provider prompt belongs to an old PTY instance");
            return Ok(true);
        }
        let decision = decide(
            prompt_id,
            principal,
            entry.capability,
            entry.consent.as_ref(),
            work,
        );
        match decision {
            PolicyDecision::Defer => {
                let Some(entry) = self
                    .provider_updates
                    .defer(prompt_id, deferred_message(work))
                else {
                    return Ok(true);
                };
                self.emit_provider_update(&entry.view);
                Ok(false)
            }
            PolicyDecision::Install { .. } => {
                let Some(mut entry) = self.provider_updates.claim_install(prompt_id) else {
                    return Ok(true);
                };
                self.emit_provider_update(&entry.view);
                self.write_provider_update_keys(&entry, &entry.install_writes)?;
                if entry
                    .capability
                    .and_then(|capability| capability.completion_signal)
                    .is_none()
                {
                    entry.view.message = "Update started, but this provider does not declare an observable completion signal; session restart is deferred.".to_string();
                    self.provider_updates.finish(
                        entry.clone(),
                        ProviderUpdateStatus::Installing,
                        entry.view.message.clone(),
                    );
                    self.emit_provider_update(&entry.view);
                }
                Ok(true)
            }
            PolicyDecision::Decline(reason) => {
                let mut entry = self.provider_updates.remove(prompt_id).unwrap_or(entry);
                entry.view.status = ProviderUpdateStatus::Declined;
                entry.view.message = format!("Update declined: {}.", decline_reason_text(reason));
                self.emit_provider_update(&entry.view);
                self.write_provider_update_keys(&entry, &entry.decline_writes)?;
                Ok(true)
            }
            PolicyDecision::AwaitConsent => Ok(false),
        }
    }

    fn start_provider_update_waiter(
        &self,
        prompt_id: String,
        receiver: &mut tokio::sync::broadcast::Receiver<crate::api::events::SeqEvent>,
    ) {
        if !self.provider_updates.mark_waiter_started(&prompt_id) {
            return;
        }
        let svc = self.clone();
        let mut receiver = receiver.resubscribe();
        std::thread::spawn(move || loop {
            match receiver.blocking_recv() {
                Ok(event)
                    if matches!(
                        event.event,
                        crate::api::events::SystemEvent::MuxStateChanged
                            | crate::api::events::SystemEvent::TabClosed { .. }
                    ) =>
                {
                    match svc.try_execute_provider_update(&prompt_id) {
                        Ok(true) | Err(_) => break,
                        Ok(false) => {}
                    }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        });
    }

    pub(crate) fn handle_provider_update_completed(&self, tab_id: &str, pty_id: &str) {
        let Some(entry) = self.provider_updates.claim_completion(tab_id, pty_id) else {
            return;
        };
        let restart_required = entry
            .capability
            .is_some_and(|capability| capability.restart_required);
        if restart_required {
            if let Err(error) = self.restart_after_provider_update(&entry) {
                self.fail_provider_update(entry, &error);
                return;
            }
        }
        let (status, message) = if restart_required {
            (
                ProviderUpdateStatus::Restarted,
                "Update completion was observed and the provider session reached readiness on its new PTY."
                    .to_string(),
            )
        } else {
            (
                ProviderUpdateStatus::Completed,
                "Provider update completion was observed.".to_string(),
            )
        };
        self.provider_updates
            .finish(entry.clone(), status, message.clone());
        let mut view = entry.view;
        view.status = status;
        view.message = message;
        self.emit_provider_update(&view);
    }

    fn provider_update_context(
        &self,
        tab_id: &str,
    ) -> Option<(String, ConsentPrincipal, WorkInFlight)> {
        let (pty_id, principal, delivery_state, delivery_attempt_active) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            let principal = consent_principal(tab.audience, tab.report_target_mesh_peer.is_some());
            let delivery_attempt_active = tab.pending_delivery.as_ref().is_some_and(|record| {
                !record.state().is_accepted() && record.attempt_provenance().is_some()
            });
            (
                tab.pty_id.clone()?,
                principal,
                tab.pending_delivery.as_ref().map(|record| record.state()),
                delivery_attempt_active,
            )
        };
        let provider_turn_active = matches!(
            self.agent_tracker.lock().get_status(&pty_id),
            TabStatus::Working
        );
        Some((
            pty_id,
            principal,
            WorkInFlight {
                delivery_state,
                delivery_attempt_active,
                provider_turn_active,
            },
        ))
    }

    fn write_provider_update_keys(
        &self,
        entry: &PendingProviderUpdate,
        writes: &[Vec<u8>],
    ) -> Result<(), String> {
        #[cfg(test)]
        if self.provider_updates.probe.lock().is_some() {
            for bytes in writes {
                self.provider_updates
                    .record(ProviderUpdateTestEffect::Wrote {
                        pty_id: entry.pty_id.clone(),
                        bytes: bytes.clone(),
                    });
            }
            return Ok(());
        }
        let pty = self.pty.lock();
        for bytes in writes {
            pty.write(&entry.pty_id, bytes)?;
        }
        Ok(())
    }

    fn emit_provider_update(&self, view: &ProviderUpdatePromptView) {
        #[cfg(test)]
        self.provider_updates
            .record(ProviderUpdateTestEffect::Emitted(view.clone()));
        crate::api::events::emit_everywhere(&self.app_handle, "provider-update-prompt", view);
    }

    fn restart_after_provider_update(&self, entry: &PendingProviderUpdate) -> Result<(), String> {
        #[cfg(test)]
        if self.provider_updates.probe.lock().is_some() {
            self.provider_updates
                .record(ProviderUpdateTestEffect::Restarted {
                    tab_id: entry.view.tab_id.clone(),
                });
            return Ok(());
        }
        self.agent_restart_observed(
            &entry.view.tab_id,
            false,
            crate::service::agent_restart::DEFAULT_GRACE_SECS,
        )
        .map(|_| ())
        .map_err(|error| format!("provider restart failed: {error:?}"))
    }

    fn fail_provider_update(&self, mut entry: PendingProviderUpdate, message: &str) {
        self.provider_updates.remove(&entry.view.prompt_id);
        entry.view.status = ProviderUpdateStatus::Failed;
        entry.view.message = message.to_string();
        self.emit_provider_update(&entry.view);
    }
}

fn consent_principal(audience: TabAudience, mesh_dispatched: bool) -> ConsentPrincipal {
    if audience == TabAudience::Automation {
        ConsentPrincipal::Unauthorized(DeclineReason::Automation)
    } else if mesh_dispatched {
        ConsentPrincipal::Unauthorized(DeclineReason::MeshDispatched)
    } else {
        ConsentPrincipal::LocalInteractive
    }
}

fn decline_reason_text(reason: DeclineReason) -> &'static str {
    match reason {
        DeclineReason::Automation => "the tab has no interactive user audience",
        DeclineReason::MeshDispatched => "mesh-dispatched agents cannot authorize installs",
        DeclineReason::Unsupported => "the provider does not support in-session updates",
        DeclineReason::StalePrompt => "the one-shot prompt authority is stale",
        DeclineReason::UserDeclined => "the local interactive user declined consent",
    }
}

fn deferred_message(work: WorkInFlight) -> String {
    let delivery = match work.delivery_state {
        Some(state) if !state.is_accepted() => format!("PendingDeliveryRecord is {state:?}"),
        _ if work.delivery_attempt_active => "a delivery attempt is active".to_string(),
        _ if work.provider_turn_active => "the provider owns an active turn".to_string(),
        _ => "work is still in flight".to_string(),
    };
    format!("Update approved and deferred because {delivery}.")
}

#[cfg(test)]
mod tests;
