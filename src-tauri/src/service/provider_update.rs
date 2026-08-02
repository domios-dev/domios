use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

use crate::agent::provider::ProviderSelfUpdateCapability;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStep {
    SurfaceConsent,
    SurfaceDecline,
    PressDecline,
    SurfaceDeferred,
    PressInstall,
    RestartObserved,
}

pub fn execution_steps(decision: PolicyDecision) -> Vec<ExecutionStep> {
    match decision {
        PolicyDecision::AwaitConsent => vec![ExecutionStep::SurfaceConsent],
        PolicyDecision::Decline(_) => {
            vec![ExecutionStep::SurfaceDecline, ExecutionStep::PressDecline]
        }
        PolicyDecision::Defer => vec![ExecutionStep::SurfaceDeferred],
        PolicyDecision::Install {
            restart_required: true,
        } => vec![ExecutionStep::PressInstall, ExecutionStep::RestartObserved],
        PolicyDecision::Install {
            restart_required: false,
        } => vec![ExecutionStep::PressInstall],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderUpdateStatus {
    AwaitingConsent,
    Declined,
    Deferred,
    Installing,
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
    waiter_started: bool,
}

#[derive(Clone, Default)]
pub struct ProviderUpdateRegistry {
    entries: Arc<Mutex<HashMap<String, PendingProviderUpdate>>>,
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

    fn update(&self, entry: PendingProviderUpdate) {
        self.insert(entry);
    }

    fn remove(&self, prompt_id: &str) -> Option<PendingProviderUpdate> {
        self.entries.lock().remove(prompt_id)
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
        let mut entry = self
            .provider_updates
            .get(prompt_id)
            .ok_or_else(|| "Provider update prompt is no longer active".to_string())?;
        if entry.view.status == ProviderUpdateStatus::Declined {
            self.provider_updates.remove(prompt_id);
            return Ok(());
        }
        if !accept {
            self.provider_updates.remove(prompt_id);
            entry.view.status = ProviderUpdateStatus::Declined;
            entry.view.message = "Update declined by the local interactive user.".to_string();
            self.emit_provider_update(&entry.view);
            self.write_provider_update_keys(&entry, &entry.decline_writes)?;
            log::info!(
                "[provider-update] consent recorded decision=decline prompt={} tab={} provider={}",
                prompt_id,
                entry.view.tab_id,
                entry.provider_id
            );
            return Ok(());
        }

        entry.consent = Some(RecordedConsent {
            prompt_id: prompt_id.to_string(),
        });
        log::info!(
            "[provider-update] consent recorded authority=local_interactive prompt={} tab={} provider={}",
            prompt_id,
            entry.view.tab_id,
            entry.provider_id
        );
        let mut receiver = self.events.subscribe();
        self.provider_updates.update(entry);
        if self.try_execute_provider_update(prompt_id)? {
            return Ok(());
        }
        self.start_provider_update_waiter(prompt_id.to_string(), &mut receiver);
        Ok(())
    }

    fn try_execute_provider_update(&self, prompt_id: &str) -> Result<bool, String> {
        let mut entry = match self.provider_updates.get(prompt_id) {
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
                entry.view.status = ProviderUpdateStatus::Deferred;
                entry.view.message = deferred_message(work);
                self.provider_updates.update(entry.clone());
                self.emit_provider_update(&entry.view);
                Ok(false)
            }
            PolicyDecision::Install { restart_required } => {
                self.provider_updates.remove(prompt_id);
                entry.view.status = ProviderUpdateStatus::Installing;
                entry.view.message = "Installing the authorized provider update.".to_string();
                self.emit_provider_update(&entry.view);
                self.write_provider_update_keys(&entry, &entry.install_writes)?;
                if restart_required {
                    self.agent_restart_observed(
                        &entry.view.tab_id,
                        false,
                        crate::service::agent_restart::DEFAULT_GRACE_SECS,
                    )
                    .map_err(|error| format!("provider restart failed: {error:?}"))?;
                }
                entry.view.status = ProviderUpdateStatus::Restarted;
                entry.view.message = if restart_required {
                    "Update installed and the provider session reached readiness on its new PTY."
                        .to_string()
                } else {
                    "Update installed; this provider does not require a session restart."
                        .to_string()
                };
                self.emit_provider_update(&entry.view);
                Ok(true)
            }
            PolicyDecision::Decline(reason) => {
                self.provider_updates.remove(prompt_id);
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
        let Some(mut entry) = self.provider_updates.get(&prompt_id) else {
            return;
        };
        if entry.waiter_started {
            return;
        }
        entry.waiter_started = true;
        self.provider_updates.update(entry);
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

    fn provider_update_context(
        &self,
        tab_id: &str,
    ) -> Option<(String, ConsentPrincipal, WorkInFlight)> {
        let (pty_id, principal, delivery_state, delivery_attempt_active) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            let principal = if tab.audience == TabAudience::Automation {
                ConsentPrincipal::Unauthorized(DeclineReason::Automation)
            } else if tab.report_target_mesh_peer.is_some() {
                ConsentPrincipal::Unauthorized(DeclineReason::MeshDispatched)
            } else {
                ConsentPrincipal::LocalInteractive
            };
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
        let pty = self.pty.lock();
        for bytes in writes {
            pty.write(&entry.pty_id, bytes)?;
        }
        Ok(())
    }

    fn emit_provider_update(&self, view: &ProviderUpdatePromptView) {
        crate::api::events::emit_everywhere(&self.app_handle, "provider-update-prompt", view);
    }

    fn fail_provider_update(&self, mut entry: PendingProviderUpdate, message: &str) {
        self.provider_updates.remove(&entry.view.prompt_id);
        entry.view.status = ProviderUpdateStatus::Failed;
        entry.view.message = message.to_string();
        self.emit_provider_update(&entry.view);
    }
}

fn decline_reason_text(reason: DeclineReason) -> &'static str {
    match reason {
        DeclineReason::Automation => "the tab has no interactive user audience",
        DeclineReason::MeshDispatched => "mesh-dispatched agents cannot authorize installs",
        DeclineReason::Unsupported => "the provider does not support in-session updates",
        DeclineReason::StalePrompt => "the one-shot prompt authority is stale",
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
mod tests {
    use super::*;

    fn capability() -> Option<ProviderSelfUpdateCapability> {
        Some(ProviderSelfUpdateCapability {
            in_session: true,
            restart_required: true,
        })
    }

    fn clear_work() -> WorkInFlight {
        WorkInFlight {
            delivery_state: Some(PendingDeliveryState::AcceptedStrong),
            delivery_attempt_active: false,
            provider_turn_active: false,
        }
    }

    #[test]
    fn unauthorized_provider_update_is_visible_and_declined() {
        let decision = decide(
            "prompt-1",
            ConsentPrincipal::Unauthorized(DeclineReason::Automation),
            capability(),
            None,
            clear_work(),
        );

        assert_eq!(
            execution_steps(decision),
            vec![ExecutionStep::SurfaceDecline, ExecutionStep::PressDecline]
        );
    }

    #[test]
    fn authorized_provider_update_restarts_after_delivery_resolution() {
        let consent = RecordedConsent {
            prompt_id: "prompt-1".to_string(),
        };
        for state in [
            PendingDeliveryState::Pending,
            PendingDeliveryState::Ingested,
            PendingDeliveryState::Unconfirmed,
        ] {
            assert_eq!(
                decide(
                    "prompt-1",
                    ConsentPrincipal::LocalInteractive,
                    capability(),
                    Some(&consent),
                    WorkInFlight {
                        delivery_state: Some(state),
                        delivery_attempt_active: false,
                        provider_turn_active: false,
                    },
                ),
                PolicyDecision::Defer
            );
        }
        for blocked in [
            WorkInFlight {
                delivery_state: Some(PendingDeliveryState::AcceptedStrong),
                delivery_attempt_active: true,
                provider_turn_active: false,
            },
            WorkInFlight {
                delivery_state: Some(PendingDeliveryState::AcceptedWeak),
                delivery_attempt_active: false,
                provider_turn_active: true,
            },
        ] {
            assert_eq!(
                decide(
                    "prompt-1",
                    ConsentPrincipal::LocalInteractive,
                    capability(),
                    Some(&consent),
                    blocked,
                ),
                PolicyDecision::Defer
            );
        }
        for state in [
            PendingDeliveryState::AcceptedStrong,
            PendingDeliveryState::AcceptedWeak,
        ] {
            assert_eq!(
                execution_steps(decide(
                    "prompt-1",
                    ConsentPrincipal::LocalInteractive,
                    capability(),
                    Some(&consent),
                    WorkInFlight {
                        delivery_state: Some(state),
                        delivery_attempt_active: false,
                        provider_turn_active: false,
                    },
                )),
                vec![ExecutionStep::PressInstall, ExecutionStep::RestartObserved]
            );
        }
    }
}
