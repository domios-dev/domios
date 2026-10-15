use super::{optimistic_agent_pty_target, public_agent_input_readiness, AuthenticatedPtyDelivery};
use crate::service::{delivery_error::DeliveryError, ServiceContext};

impl ServiceContext {
    pub(crate) fn pin_agent_delivery_binding(
        &self,
        claim: &crate::service::pty_ownership::TabPtyClaim,
        payload: &mut crate::service::messenger::AgentDeliveryPayload,
    ) -> Result<(), String> {
        let tab_id = claim.tab_id();
        let restored = {
            let mux = self.mux.read();
            mux.resolve_tab_id(tab_id)
                .and_then(|id| mux.get_tab(&id))
                .filter(|tab| tab.restored_from_snapshot && !tab.agent_booting)
                .is_some()
        };
        if restored {
            self.materialize_restored_agent_with_claim(claim)?;
        }
        let binding = self
            .queued_agent_binding(tab_id)
            .ok_or_else(|| "agent_unavailable: recipient has no active PTY launch".to_string())?;
        if payload
            .expected_pty_id
            .as_deref()
            .is_some_and(|expected| Some(expected) != binding.0.as_deref())
            || payload
                .expected_session_id
                .as_deref()
                .is_some_and(|expected| Some(expected) != binding.1.as_deref())
            || payload
                .expected_binding_nonce
                .as_deref()
                .is_some_and(|expected| expected != binding.2)
        {
            return Err("agent_unavailable: recipient session binding changed".into());
        }
        payload.expected_pty_id = binding.0;
        payload.expected_session_id = binding.1;
        payload.expected_binding_nonce = Some(binding.2);
        Ok(())
    }

    pub(crate) fn agent_delivery_binding_matches(
        &self,
        tab_id: &str,
        payload: &crate::service::messenger::AgentDeliveryPayload,
    ) -> bool {
        let Some(binding) = self.queued_agent_binding(tab_id) else {
            return false;
        };
        payload_matches_binding(payload, &binding)
    }

    fn queued_agent_binding(
        &self,
        tab_id: &str,
    ) -> Option<(Option<String>, Option<String>, String)> {
        let mux = self.mux.read();
        let resolved = mux.resolve_tab_id(tab_id)?;
        delivery_binding(mux.get_tab(&resolved)?)
    }

    pub(crate) fn send_queued_agent_payload(
        &self,
        tab_id: &str,
        payload: &crate::service::messenger::AgentDeliveryPayload,
    ) -> Result<(), String> {
        let Some(_expected_binding_nonce) = payload.expected_binding_nonce.as_deref() else {
            return Err(DeliveryError::TabBusyRefused {
                tab: tab_id.to_string(),
                detail: "queued agent delivery has no binding authority".into(),
            }
            .ledger_message());
        };
        let (pty_id, provider_id) = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| "agent_unavailable: recipient missing".to_string())?;
            let tab = mux
                .get_tab(&resolved)
                .ok_or_else(|| "agent_unavailable: recipient missing".to_string())?;
            queued_delivery_target(tab, payload).map_err(|detail| {
                DeliveryError::TabBusyRefused {
                    tab: tab_id.to_string(),
                    detail,
                }
                .ledger_message()
            })?
        };
        log::debug!(
            "[agent-delivery] tab={tab_id} provider={provider_id} pty={pty_id} branch=resolved"
        );
        #[cfg(any(test, feature = "test-support"))]
        if let Some(result) =
            crate::service::agent_delivery_outbox::test_boundary::submit(tab_id, payload)
        {
            return result;
        }
        let permit = {
            let pty = self.pty.lock();
            if pty.get_session(&pty_id).is_none() {
                return Err(format!(
                    "agent_unavailable: recipient PTY missing: {pty_id}"
                ));
            }
            pty.issue_guarded_input_permit(&pty_id).map_err(|detail| {
                DeliveryError::TabBusyRefused {
                    tab: tab_id.to_string(),
                    detail,
                }
                .ledger_message()
            })?
        };
        let delivery = AuthenticatedPtyDelivery {
            pty_id,
            provider_id,
            permit,
        };
        match &payload.transport {
            crate::service::messenger::QueuedAgentPayload::Text { text } => self
                .send_authenticated_delivery_with_options(
                    delivery,
                    text,
                    crate::service::pty_io::submit_transaction::PtySubmitEntry::Messenger,
                    true,
                ),
            crate::service::messenger::QueuedAgentPayload::RoutedFrame {
                opening_tag,
                body,
                suffix,
            } => self.send_authenticated_delivery_with_routed_frame(
                delivery,
                codeterm_wire::domios::DomiosFrame {
                    opening_tag: opening_tag.clone(),
                    body: body.clone(),
                    suffix: suffix.clone(),
                },
            ),
        }
    }
}

fn delivery_binding(
    tab: &crate::multiplexer::types::Tab,
) -> Option<(Option<String>, Option<String>, String)> {
    if tab.agent_type.is_none() || tab.is_chat_backend() {
        return None;
    }
    let pty_id = if tab.agent_booting
        && matches!(
            tab.boot_stage,
            Some(codeterm_types::pty::BootStage::PtyStarting)
        ) {
        None
    } else {
        tab.pty_id.clone()
    };
    if tab.restored_from_snapshot
        || tab.boot_stage == Some(codeterm_types::pty::BootStage::SpawnFailed)
    {
        return None;
    }
    let nonce = tab.session_binding_nonce.clone()?;
    let provisional = tab.provisional_launch.as_ref().is_some_and(|launch| {
        Some(&launch.pty_id) == pty_id.as_ref() && launch.binding_nonce == nonce
    });
    (tab.agent_booting
        || provisional
        || tab.launch_session_authenticated()
        || tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady))
    .then(|| (pty_id, tab.agent_session_id.clone(), nonce))
}

fn payload_matches_binding(
    payload: &crate::service::messenger::AgentDeliveryPayload,
    binding: &(Option<String>, Option<String>, String),
) -> bool {
    payload
        .expected_pty_id
        .as_ref()
        .is_none_or(|pty| Some(pty) == binding.0.as_ref())
        && payload
            .expected_session_id
            .as_ref()
            .is_none_or(|session| Some(session) == binding.1.as_ref())
        && payload.expected_binding_nonce.as_deref() == Some(binding.2.as_str())
}

fn queued_delivery_target(
    tab: &crate::multiplexer::types::Tab,
    payload: &crate::service::messenger::AgentDeliveryPayload,
) -> Result<(String, String), String> {
    let binding =
        delivery_binding(tab).ok_or_else(|| "recipient launch is unavailable".to_string())?;
    if !payload_matches_binding(payload, &binding) {
        return Err("recipient launch changed".into());
    }
    if binding.0.is_none() {
        return Err("recipient launch is awaiting PTY registration".into());
    }
    if binding.1.is_some() && !tab.launch_session_authenticated() {
        return Err("recipient launch is awaiting session binding".into());
    }
    match public_agent_input_readiness(tab) {
        Some(Ok(())) => optimistic_agent_pty_target(tab),
        Some(Err(error)) => Err(error),
        None if tab.launch_session_authenticated() && !tab.agent_booting => {
            optimistic_agent_pty_target(tab)
        }
        None => Err("recipient launch is awaiting readiness".into()),
    }
}

#[cfg(test)]
mod tests;
