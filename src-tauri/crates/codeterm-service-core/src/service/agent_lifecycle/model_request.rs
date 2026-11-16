use super::ModelSwitchError;
use crate::service::{MuxEmitCause, ServiceContext};

impl ServiceContext {
    pub fn record_live_model_request(
        &self,
        tab_id: &str,
        provider: &str,
        session_id: &str,
        requested_model: &str,
        effort: Option<&str>,
    ) -> Result<(), ModelSwitchError> {
        self.record_model_request(tab_id, provider, session_id, requested_model, effort, None)
    }

    pub fn record_model_request(
        &self,
        tab_id: &str,
        provider: &str,
        session_id: &str,
        requested_model: &str,
        effort: Option<&str>,
        boundary: Option<codeterm_types::agent::SessionModelTranscriptBoundary>,
    ) -> Result<(), ModelSwitchError> {
        let updated = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return Err(ModelSwitchError::NotFound(format!(
                    "Tab {tab_id} not found"
                )));
            };
            if tab.agent_type.as_deref() != Some(provider)
                || tab.agent_session_id.as_deref() != Some(session_id)
                || tab.session_binding_nonce.is_none()
                || tab.session_binding_nonce != tab.reported_session_binding_nonce
            {
                return Err(ModelSwitchError::NotSwitchable(
                    "the tab's verified session binding changed during the live model switch"
                        .into(),
                ));
            }
            record_live_request_identity(tab, requested_model, effort);
            if let Some(boundary) = boundary {
                record_pending_turn(tab, session_id, requested_model, boundary);
            }
            true
        };
        if updated {
            if let Err(error) = crate::service::session_persist::persist_session_after_mutation(
                self,
                crate::service::session_persist::SessionPersistUrgency::Prompt,
            ) {
                log::warn!(
                    "[agent-model] tab={tab_id} failed to persist the live model request: {error}"
                );
            }
            self.persist_current_attempt_session_model(tab_id);
            self.emit_state_changed(MuxEmitCause::Model);
        }
        Ok(())
    }
}

pub fn record_live_request_identity(
    tab: &mut crate::multiplexer::types::Tab,
    model: &str,
    effort: Option<&str>,
) {
    tab.agent_requested_model = Some(model.to_owned());
    tab.agent_provider_resolved_model = Some(model.to_owned());
    tab.restorable_launch.captured = true;
    tab.restorable_launch.requested_model = Some(model.to_owned());
    tab.restorable_launch.model = Some(model.to_owned());
    tab.restorable_launch.requested_reasoning_effort = effort.map(str::to_owned);
    tab.restorable_launch.resolved_reasoning_effort = effort.map(str::to_owned);
    crate::service::reasoning_effort::persist_launch_identity(tab, effort, effort);
}

pub fn record_pending_turn(
    tab: &mut crate::multiplexer::types::Tab,
    session_id: &str,
    requested: &str,
    boundary: codeterm_types::agent::SessionModelTranscriptBoundary,
) {
    tab.pending_model_switch_observation =
        Some(crate::multiplexer::types::PendingModelSwitchObservation {
            session_id: session_id.into(),
            launched_model: requested.into(),
            transcript_boundary: boundary,
        });
}
