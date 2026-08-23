use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

use crate::agent::provider::{ProviderSelfUpdateCapability, ProviderUpdateCompletionSignal};
use crate::agent::status::TabStatus;
use crate::notification::dedupe::{provider_update_condition, ConditionTransition};
use crate::notification::push::tab_scope;
use crate::multiplexer::types::{PendingDeliveryState, TabAudience};

use super::ServiceContext;

const RUNNER_DESCRIPTION_PREFIX: &str = "codeterm:provider-update:";

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

/// Why a provider's update signal did or did not reach the user. Without this
/// a swallowed prompt is indistinguishable from one that was never signalled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderUpdatePromptOutcome {
    /// The tab is gone, or has no PTY to answer on.
    TabUnavailable,
    /// This tab IS an update tab: its install keys were submitted there.
    InstallSubmitted,
    /// This tab is an update tab whose prompt is no longer taking an install.
    UpdateTabSettled,
    /// This source tab already owns a prompt that has not terminated.
    SourceAlreadyPrompted,
    Raised(ProviderUpdateStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUpdateStatus {
    AwaitingConsent,
    Declined,
    Deferred,
    Installing,
    CompletionUnavailable,
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
    runner_tab_id: Option<String>,
    runner_pty_id: Option<String>,
    install_submitted: bool,
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

    fn has_open_source(&self, tab_id: &str, provider_id: &str) -> bool {
        self.entries.lock().values().any(|entry| {
            entry.view.tab_id == tab_id
                && entry.provider_id == provider_id
                && entry.phase != ProviderUpdatePhase::Terminal
        })
    }

    fn get(&self, prompt_id: &str) -> Option<PendingProviderUpdate> {
        self.entries.lock().get(prompt_id).cloned()
    }

    fn acknowledge_terminal(&self, prompt_id: &str) -> Result<(), String> {
        let mut entries = self.entries.lock();
        let entry = entries
            .get(prompt_id)
            .ok_or_else(|| "Provider update prompt is no longer active".to_string())?;
        if entry.phase != ProviderUpdatePhase::Terminal {
            return Err("Only terminal provider update prompts can be acknowledged".to_string());
        }
        entries.remove(prompt_id);
        Ok(())
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
            entry.runner_tab_id.as_deref() == Some(tab_id)
                && entry.runner_pty_id.as_deref() == Some(pty_id)
                && entry.phase == ProviderUpdatePhase::Installing
                && entry.install_submitted
                && entry
                    .capability
                    .and_then(|capability| capability.completion_signal)
                    == Some(ProviderUpdateCompletionSignal::ProviderClassification)
        })?;
        entry.phase = ProviderUpdatePhase::Completing;
        Some(entry.clone())
    }

    fn bind_runner(&self, prompt_id: &str, tab_id: String) -> bool {
        let mut entries = self.entries.lock();
        let Some(entry) = entries.get_mut(prompt_id) else {
            return false;
        };
        if entry.phase != ProviderUpdatePhase::Installing {
            return false;
        }
        entry.runner_tab_id = Some(tab_id);
        true
    }

    /// `(prompt_id, update_tab_id)` for every prompt still owning a live-looking
    /// update tab — the caller resolves aliveness against the multiplexer.
    fn open_runner_tabs(&self) -> Vec<(String, String)> {
        self.entries
            .lock()
            .values()
            .filter(|entry| entry.phase != ProviderUpdatePhase::Terminal)
            .filter_map(|entry| {
                entry
                    .runner_tab_id
                    .clone()
                    .map(|tab_id| (entry.view.prompt_id.clone(), tab_id))
            })
            .collect()
    }

    /// Which prompt owns this tab as its update tab. Authoritative: the registry
    /// knows what it bound, so nothing here depends on a tab's description text.
    /// Phase-independent on purpose — a settled update tab must still not be
    /// mistaken for a source tab, exactly as before.
    fn prompt_for_runner_tab(&self, tab_id: &str) -> Option<String> {
        self.entries
            .lock()
            .values()
            .find(|entry| entry.runner_tab_id.as_deref() == Some(tab_id))
            .map(|entry| entry.view.prompt_id.clone())
    }

    fn runner_tab(&self, prompt_id: &str) -> Option<String> {
        let entries = self.entries.lock();
        let entry = entries.get(prompt_id)?;
        if entry.phase == ProviderUpdatePhase::Terminal {
            return None;
        }
        entry.runner_tab_id.clone()
    }

    fn claim_runner_prompt(
        &self,
        prompt_id: &str,
        tab_id: &str,
        pty_id: &str,
        install_writes: Vec<Vec<u8>>,
    ) -> Option<PendingProviderUpdate> {
        let mut entries = self.entries.lock();
        let entry = entries.get_mut(prompt_id)?;
        if entry.phase != ProviderUpdatePhase::Installing
            || entry.runner_tab_id.as_deref() != Some(tab_id)
        {
            return None;
        }
        entry.runner_pty_id = Some(pty_id.to_string());
        entry.install_writes = install_writes;
        entry.install_submitted = true;
        let mut runner = entry.clone();
        runner.pty_id = pty_id.to_string();
        Some(runner)
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
    ) -> ProviderUpdatePromptOutcome {
        let _ = self.reconcile_closed_update_tabs();
        let Some((pty_id, principal, work)) = self.provider_update_context(tab_id) else {
            log::debug!("[provider-update] ignored tab={tab_id}: no tab or no PTY");
            return ProviderUpdatePromptOutcome::TabUnavailable;
        };
        if let Some(prompt_id) = self.provider_update_runner_prompt_id(tab_id) {
            let Some(entry) = self.provider_updates.claim_runner_prompt(
                &prompt_id,
                tab_id,
                &pty_id,
                install_writes,
            ) else {
                log::debug!("[provider-update] ignored tab={tab_id}: update tab already settled");
                return ProviderUpdatePromptOutcome::UpdateTabSettled;
            };
            if let Err(error) = self.write_provider_update_keys(&entry, &entry.install_writes) {
                self.fail_provider_update(entry, &error);
            }
            return ProviderUpdatePromptOutcome::InstallSubmitted;
        }
        if self.provider_updates.has_open_source(tab_id, provider_id) {
            log::debug!("[provider-update] ignored tab={tab_id}: a prompt is already open");
            return ProviderUpdatePromptOutcome::SourceAlreadyPrompted;
        }
        // "An update is available" stays true until it is installed. Announce
        // the transition; a later observation of the same state is not news.
        if self.announce_provider_update_available(tab_id, provider_id)
            == ConditionTransition::AlreadyAnnounced
        {
            return;
        }
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
            other => {
                log::debug!("[provider-update] ignored tab={tab_id}: unconsented {other:?}");
                return ProviderUpdatePromptOutcome::TabUnavailable;
            }
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
            runner_tab_id: None,
            runner_pty_id: None,
            install_submitted: false,
        };
        self.provider_updates.insert(entry.clone());
        self.emit_provider_update(&entry.view);
        if matches!(decision, PolicyDecision::AwaitConsent) {
            self.create_provider_update_notification(
                prompt_id,
                &entry.view.provider_name,
                entry.view.message.clone(),
                tab_id.to_string(),
            );
        }
        if let Err(error) = self.write_provider_update_keys(&entry, &entry.decline_writes) {
            self.fail_provider_update(entry.clone(), &error);
        }
        if matches!(decision, PolicyDecision::Decline(_)) {
            log::warn!(
                "[provider-update] declined tab={} provider={} reason={}",
                tab_id,
                provider_id,
                entry.view.message
            );
        }
        ProviderUpdatePromptOutcome::Raised(status)
    }

    pub fn list_provider_update_prompts(&self) -> Vec<ProviderUpdatePromptView> {
        let _ = self.reconcile_closed_update_tabs();
        self.provider_updates.list()
    }

    pub fn respond_provider_update_prompt(
        &self,
        prompt_id: &str,
        accept: bool,
    ) -> Result<(), String> {
        let _ = self.reconcile_closed_update_tabs();
        // One press, one tab: a repeat press lands the user on the update
        // already running rather than racing a second installer against it.
        if accept {
            if let Some(tab_id) = self.provider_updates.runner_tab(prompt_id) {
                let workspace_id = {
                    let mux = self.mux.read();
                    mux.find_workspace_for_tab(&tab_id)
                }
                .ok_or_else(|| "The update tab is no longer in a workspace".to_string())?;
                return self.focus_provider_update_tab(&workspace_id, &tab_id);
            }
        }
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
                self.resolve_provider_update_notification(prompt_id);
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
                self.resolve_provider_update_notification(prompt_id);
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

    pub fn acknowledge_provider_update_prompt(&self, prompt_id: &str) -> Result<(), String> {
        self.provider_updates.acknowledge_terminal(prompt_id)
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
                if entry
                    .capability
                    .and_then(|capability| capability.completion_signal)
                    .is_none()
                {
                    entry.view.message = "Update started, but this provider does not declare an observable completion signal; CodeTerm will not restart this session.".to_string();
                    self.provider_updates.finish(
                        entry.clone(),
                        ProviderUpdateStatus::CompletionUnavailable,
                        entry.view.message.clone(),
                    );
                    entry.view.status = ProviderUpdateStatus::CompletionUnavailable;
                    self.emit_provider_update(&entry.view);
                    return Ok(true);
                }
                self.start_provider_update_runner(&entry)?;
                Ok(true)
            }
            PolicyDecision::Decline(reason) => {
                let message = format!("Update declined: {}.", decline_reason_text(reason));
                self.provider_updates.finish(
                    entry.clone(),
                    ProviderUpdateStatus::Declined,
                    message.clone(),
                );
                let mut entry = entry;
                entry.view.status = ProviderUpdateStatus::Declined;
                entry.view.message = message;
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

    fn announce_provider_update_available(
        &self,
        tab_id: &str,
        provider_id: &str,
    ) -> ConditionTransition {
        self.notif
            .lock()
            .observe_condition_present(&tab_scope(tab_id), &provider_update_condition(provider_id))
    }

    pub(crate) fn handle_provider_update_completed(&self, tab_id: &str, pty_id: &str) {
        let Some(entry) = self.provider_updates.claim_completion(tab_id, pty_id) else {
            return;
        };
        // The condition resolved, so a genuinely new update announces again.
        self.notif.lock().observe_condition_absent(
            &tab_scope(&entry.view.tab_id),
            &provider_update_condition(&entry.provider_id),
        );
        let restart_required = entry
            .capability
            .is_some_and(|capability| capability.restart_required);
        let (restarted, deferred) = if restart_required {
            match self.restart_after_provider_update(&entry) {
                Ok(result) => result,
                Err(error) => {
                    self.fail_provider_update(entry, &error);
                    return;
                }
            }
        } else {
            (0, 0)
        };
        let (status, message) = if restart_required {
            (
                ProviderUpdateStatus::Restarted,
                format!(
                    "Update completed. Restarted {restarted} idle session(s); left {deferred} active session(s) running."
                ),
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

    pub(crate) fn provider_update_runner_is_installing(&self, tab_id: &str, pty_id: &str) -> bool {
        self.provider_updates.entries.lock().values().any(|entry| {
            entry.phase == ProviderUpdatePhase::Installing
                && entry.install_submitted
                && entry.runner_tab_id.as_deref() == Some(tab_id)
                && entry.runner_pty_id.as_deref() == Some(pty_id)
        })
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

    fn restart_after_provider_update(
        &self,
        entry: &PendingProviderUpdate,
    ) -> Result<(usize, usize), String> {
        #[cfg(test)]
        if self.provider_updates.probe.lock().is_some() {
            self.provider_updates
                .record(ProviderUpdateTestEffect::Restarted {
                    tab_id: entry.view.tab_id.clone(),
                });
            return Ok((1, 0));
        }
        // The update tab stays open and unrestarted: it is the user's evidence
        // of what the update actually did.
        let candidates: Vec<String> = {
            let mux = self.mux.read();
            mux.list_workspaces()
                .iter()
                .flat_map(|workspace| workspace.tab_ids.iter())
                .filter_map(|tab_id| mux.get_tab(tab_id))
                .filter(|tab| {
                    !tab.hidden
                        && tab.agent_type.as_deref() == Some(entry.provider_id.as_str())
                        && Some(tab.id.as_str()) != entry.runner_tab_id.as_deref()
                })
                .map(|tab| tab.id.clone())
                .collect()
        };
        let mut restarted = 0;
        let mut deferred = 0;
        for tab_id in candidates {
            let restart_safe = self
                .provider_update_context(&tab_id)
                .is_some_and(|(_, _, work)| work.restart_safe());
            if !restart_safe {
                deferred += 1;
                continue;
            }
            self.agent_restart_observed(
                &tab_id,
                false,
                crate::service::agent_restart::DEFAULT_GRACE_SECS,
            )
            .map_err(|error| format!("provider restart failed for {tab_id}: {error:?}"))?;
            restarted += 1;
        }
        Ok((restarted, deferred))
    }

    /// The update runs in an ordinary user tab in General so the user watches it
    /// happen and reads any failure where it happened.
    fn start_provider_update_runner(&self, entry: &PendingProviderUpdate) -> Result<(), String> {
        let workspace_id = {
            let mux = self.mux.read();
            crate::service::general_agent::general_workspace_id(&mux)
        }
        .ok_or_else(|| "The General workspace is unavailable for the update tab".to_string())?;
        let tab_id = self.spawn_provider_update_tab(entry, &workspace_id)?;
        if !self
            .provider_updates
            .bind_runner(&entry.view.prompt_id, tab_id.clone())
        {
            return Err("Provider update authority expired before the update tab launched".into());
        }
        self.focus_provider_update_tab(&workspace_id, &tab_id)?;
        #[cfg(test)]
        self.probe_provider_update_runner_prompt(&entry.view.prompt_id, &tab_id)?;
        Ok(())
    }

    fn spawn_provider_update_tab(
        &self,
        entry: &PendingProviderUpdate,
        workspace_id: &str,
    ) -> Result<String, String> {
        let spec = provider_update_tab_spec(entry, workspace_id);
        #[cfg(test)]
        if self.provider_updates.probe.lock().is_some() {
            let mut mux = self.mux.write();
            let tab_id = mux.create_tab(workspace_id)?;
            let _ = mux.set_tab_description(&tab_id, spec.description.clone());
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                tab.audience = spec.audience;
                tab.agent_type = Some(entry.provider_id.clone());
                tab.pty_id = Some(format!("pty-{tab_id}"));
            }
            return Ok(tab_id);
        }
        let handle = self.spawn_tab(spec)?;
        Ok(handle.tab_id)
    }

    /// Stands in for the reactor observing the provider's own update prompt
    /// inside the freshly raised tab; the PTY process itself is what the probe
    /// replaces, never the tab lifecycle under test.
    #[cfg(test)]
    fn probe_provider_update_runner_prompt(
        &self,
        prompt_id: &str,
        tab_id: &str,
    ) -> Result<(), String> {
        if self.provider_updates.probe.lock().is_none() {
            return Ok(());
        }
        let pty_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pty_id.clone())
            .ok_or_else(|| "Update tab has no PTY".to_string())?;
        let install_writes = self
            .provider_updates
            .get(prompt_id)
            .map(|entry| entry.install_writes.clone())
            .unwrap_or_default();
        let runner = self
            .provider_updates
            .claim_runner_prompt(prompt_id, tab_id, &pty_id, install_writes)
            .ok_or_else(|| "Provider update test runner was not claimed".to_string())?;
        self.write_provider_update_keys(&runner, &runner.install_writes)
    }

    /// Focus follows tab creation directly: workspace, workspace-active tab, and
    /// global focus are set in one multiplexer write, then announced.
    fn focus_provider_update_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        {
            let mut mux = self.mux.write();
            mux.set_active_tab(workspace_id, tab_id)?;
            mux.set_active_workspace(workspace_id)?;
            mux.set_focused_tab(tab_id)?;
        }
        self.emit_state_changed();
        self.emit_tab_focus_changed(tab_id);
        Ok(())
    }

    /// A closed update tab is the only terminating signal a visible run needs:
    /// its prompt fails loudly instead of pinning the source tab forever.
    /// Returns the prompts it terminated, so callers and tests can distinguish
    /// "nothing to reconcile" from "reconciled but did not terminate".
    fn reconcile_closed_update_tabs(&self) -> Vec<String> {
        let open = self.provider_updates.open_runner_tabs();
        if open.is_empty() {
            return Vec::new();
        }
        let closed: Vec<String> = {
            let mux = self.mux.read();
            open.into_iter()
                .filter(|(_, tab_id)| mux.get_tab(tab_id).is_none())
                .map(|(prompt_id, _)| prompt_id)
                .collect()
        };
        let mut terminated = Vec::new();
        for prompt_id in closed {
            if let Some(entry) = self.provider_updates.get(&prompt_id) {
                self.fail_provider_update(
                    entry,
                    "The update tab was closed before the update reported completion",
                );
                terminated.push(prompt_id);
            }
        }
        terminated
    }

    fn provider_update_runner_prompt_id(&self, tab_id: &str) -> Option<String> {
        self.provider_updates.prompt_for_runner_tab(tab_id)
    }

    fn fail_provider_update(&self, mut entry: PendingProviderUpdate, message: &str) {
        self.provider_updates.finish(
            entry.clone(),
            ProviderUpdateStatus::Failed,
            message.to_string(),
        );
        entry.view.status = ProviderUpdateStatus::Failed;
        entry.view.message = message.to_string();
        self.emit_provider_update(&entry.view);
    }
}

fn provider_update_tab_description(prompt_id: &str) -> String {
    format!("{RUNNER_DESCRIPTION_PREFIX}{prompt_id}")
}

fn provider_update_tab_spec(
    entry: &PendingProviderUpdate,
    workspace_id: &str,
) -> crate::service::spawn::SpawnTabSpec {
    crate::service::spawn::SpawnTabSpec {
        workspace_id: Some(workspace_id.to_string()),
        title: Some(format!("Updating {}", entry.view.provider_name)),
        description: Some(provider_update_tab_description(&entry.view.prompt_id)),
        visibility: crate::service::spawn::TabVisibility::Visible,
        audience: TabAudience::User,
        command: crate::service::spawn::TabCommand::Agent(crate::service::spawn::AgentSpawnSpec {
            provider: entry.provider_id.clone(),
            ..Default::default()
        }),
        ..Default::default()
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
