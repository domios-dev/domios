use super::*;
use crate::agent::provider::ModelSwitchMode;
use crate::multiplexer::types::TabBacking;
use crate::service::agent_lifecycle::{
    ModelSwitchConfirmation, ModelSwitchError, ModelSwitchOutcome,
};

impl ServiceContext {
    pub fn record_product_provider_switch(
        &self,
        outcome: &codeterm_wire::api_contracts::ProviderSwitchOutcome,
    ) {
        let current = self.mux.read().get_tab(&outcome.tab_id).is_some_and(|tab| {
            tab.agent_type.as_deref() == Some(outcome.provider.as_str())
                && tab.agent_session_id.as_deref() == Some(outcome.session_id.as_str())
        });
        if !current || outcome.previous_provider == outcome.provider {
            return;
        }
        self.record_product_event(Event::AgentProviderChanged {
            provider_id: ProviderId::from_catalog(&outcome.provider).unwrap_or(ProviderId::Other),
            outcome: if outcome.binding_restored && outcome.handoff_delivered {
                codeterm_analytics::ProviderChangeOutcome::Applied
            } else {
                codeterm_analytics::ProviderChangeOutcome::Pending
            },
        });
    }

    pub fn record_product_model_switch(
        &self,
        result: &Result<ModelSwitchOutcome, ModelSwitchError>,
        source_nonce: Option<&str>,
    ) {
        let Ok(outcome) = result else { return };
        if outcome.previous_model.as_deref() == Some(outcome.launched_model.as_str()) {
            return;
        }
        let confirmation = match &outcome.confirmation {
            ModelSwitchConfirmation::Observed { .. } => {
                codeterm_analytics::ModelChangeOutcome::Observed
            }
            ModelSwitchConfirmation::Pending => codeterm_analytics::ModelChangeOutcome::Pending,
            _ => return,
        };
        let mode = match outcome.mechanism {
            ModelSwitchMode::Live | ModelSwitchMode::LiveCommand => {
                codeterm_analytics::ModelChangeMode::Live
            }
            ModelSwitchMode::RestartResume => codeterm_analytics::ModelChangeMode::Restart,
            ModelSwitchMode::NextTurn => codeterm_analytics::ModelChangeMode::NextTurn,
            ModelSwitchMode::Unsupported => return,
        };
        let current = self.mux.read().get_tab(&outcome.tab_id).is_some_and(|tab| {
            matches!(tab.backing, TabBacking::Pty)
                && tab.agent_type.as_deref() == Some(outcome.provider.as_str())
                && tab.agent_session_id.as_deref().unwrap_or_default() == outcome.session_id
                && if let Some(restart) = &outcome.restart {
                    restart.agent_ready
                        && restart.binding_restored
                        && !restart.source_account_restored
                        && !restart.account_target_refused
                        && restart.new_pty_id.is_some()
                        && tab.pty_id == restart.new_pty_id
                } else {
                    source_nonce.is_some()
                        && tab.session_binding_nonce.as_deref() == source_nonce
                        && tab.reported_session_binding_nonce == tab.session_binding_nonce
                }
        });
        if current {
            self.record_product_event(Event::AgentModelChanged {
                mode,
                outcome: confirmation,
            });
        }
    }
}
