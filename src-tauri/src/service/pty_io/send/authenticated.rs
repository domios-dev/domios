use crate::service::delivery_error::DeliveryError;
use crate::service::ServiceContext;

pub(super) fn authenticated_pty_target(
    tab: &crate::multiplexer::types::Tab,
    expected_session_id: &str,
    expected_binding_nonce: &str,
) -> Result<(String, String), String> {
    let target = optimistic_agent_pty_target(tab)?;
    let authenticated = tab.agent_session_id.as_deref() == Some(expected_session_id)
        && tab.session_binding_nonce.as_deref() == Some(expected_binding_nonce)
        && tab.reported_session_binding_nonce.as_deref() == Some(expected_binding_nonce);
    if !authenticated {
        return Err("agent_unavailable: planner session is stale".into());
    }
    Ok(target)
}

pub(super) fn optimistic_agent_pty_target(
    tab: &crate::multiplexer::types::Tab,
) -> Result<(String, String), String> {
    if tab.agent_type.is_none() {
        return Err("agent_unavailable: recipient agent is not accepting input".into());
    }
    match &tab.backing {
        crate::multiplexer::types::TabBacking::Pty => Ok((
            tab.pty_id
                .clone()
                .ok_or_else(|| "agent_unavailable: recipient PTY missing".to_string())?,
            tab.agent_type.clone().unwrap_or_default(),
        )),
        crate::multiplexer::types::TabBacking::ChatBackend { .. } => {
            Err("agent_unavailable: recipient is not PTY-backed".into())
        }
    }
}

#[derive(Clone)]
struct AuthenticatedPtyDelivery {
    pty_id: String,
    provider_id: String,
    permit: crate::pty::write_queue::GuardedInputPermit,
    submit_claim: super::super::submit_transaction::SubmitCycleClaim,
}

#[cfg(test)]
pub(super) fn submit_authenticated_pty_input<T>(
    tab: &crate::multiplexer::types::Tab,
    expected_session_id: &str,
    expected_binding_nonce: &str,
    text: &str,
    submit: impl FnOnce(&str, &str, &str) -> Result<T, String>,
) -> Result<T, String> {
    let (pty_id, provider_id) =
        authenticated_pty_target(tab, expected_session_id, expected_binding_nonce)?;
    submit(&pty_id, &provider_id, text)
}

impl ServiceContext {
    fn authenticated_pty_delivery(
        &self,
        tab_id: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
    ) -> Result<AuthenticatedPtyDelivery, String> {
        let (pty_id, provider_id) = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| "agent_unavailable: planner Tab missing".to_string())?;
            let tab = mux
                .get_tab(&resolved)
                .ok_or_else(|| "agent_unavailable: planner Tab missing".to_string())?;
            authenticated_pty_target(tab, expected_session_id, expected_binding_nonce)?
        };
        let submit_claim = super::super::submit_transaction::claim_submit_cycle(&pty_id);
        let permit = self.pty.lock().issue_guarded_input_permit(&pty_id)?;
        Ok(AuthenticatedPtyDelivery {
            pty_id,
            provider_id,
            permit,
            submit_claim,
        })
    }

    pub(crate) fn send_to_agent_when_ready_detached(&self, tab_id: &str, text: &str) {
        let pty_id = match self.resolve_pty_id(tab_id) {
            Ok(pty_id) => pty_id,
            Err(error) => {
                log::warn!("[send-async] agent delivery to Tab {tab_id} failed: {error}");
                crate::api::events::emit_everywhere(
                    &self.app_handle,
                    "chat-send-failed",
                    serde_json::json!({"tab_id": tab_id, "error": error}),
                );
                return;
            }
        };
        let svc = self.clone();
        let tab = tab_id.to_string();
        let text = text.to_string();
        super::ingress::spawn_with_submit_claim(Some(pty_id.clone()), move |submit_claim| {
            let submit_claim = submit_claim.expect("resolved PTY must own an ingress claim");
            std::thread::spawn(move || {
                let pty_claim = svc.pty_ownership.acquire(&tab);
                let current_pty_id = {
                    let mux = svc.mux.read();
                    mux.get_tab(&tab)
                        .filter(|tab| tab.agent_type.is_some())
                        .ok_or_else(|| {
                            "agent_unavailable: recipient is no longer an agent".to_string()
                        })
                        .and_then(|tab| {
                            tab.pty_id.clone().ok_or_else(|| {
                                "agent_unavailable: recipient PTY missing".to_string()
                            })
                        })
                };
                let result = current_pty_id.and_then(|current_pty_id| {
                    if current_pty_id != pty_id {
                        return Err(format!(
                            "agent_unavailable: PTY changed after input ingress: {pty_id} -> {current_pty_id}"
                        ));
                    }
                    svc.send_text_to_pty_smart_delivery_claimed(
                        &pty_id,
                        &text,
                        true,
                        submit_claim,
                    )
                    .map_err(|error| error.ledger_message())
                });
                drop(pty_claim);
                if let Err(error) = result {
                    if super::delivery_is_unknown(&error) {
                        log::warn!("[send-async] agent delivery to Tab {tab} remains unknown");
                        return;
                    }
                    log::warn!("[send-async] agent delivery to Tab {tab} failed: {error}");
                    crate::api::events::emit_everywhere(
                        &svc.app_handle,
                        "chat-send-failed",
                        serde_json::json!({"tab_id": &tab, "error": &error}),
                    );
                }
            });
        });
    }

    pub(crate) fn send_to_authenticated_agent_session(
        &self,
        tab_id: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
        text: &str,
    ) -> Result<(), String> {
        let delivery =
            self.authenticated_pty_delivery(tab_id, expected_session_id, expected_binding_nonce)?;
        self.send_authenticated_delivery_with_options(
            delivery,
            text,
            super::super::submit_transaction::PtySubmitEntry::Planner,
            true,
        )
    }

    fn send_authenticated_delivery_with_options(
        &self,
        delivery: AuthenticatedPtyDelivery,
        text: &str,
        entry: super::super::submit_transaction::PtySubmitEntry,
        verify_submit: bool,
    ) -> Result<(), String> {
        let strategy = self
            .agent_registry
            .with_provider(&delivery.provider_id, |provider| {
                provider.verify_submit_strategy()
            })
            .ok_or_else(|| "agent_unavailable: unknown planner provider".to_string())?;
        super::super::submit_transaction::submit_entry(entry, text, |text| {
            self.send_text_verified_guarded_claimed(
                &delivery.pty_id,
                text,
                true,
                strategy,
                delivery.permit.clone(),
                crate::service::inject_verify::VerifyOpts {
                    require_prompt_first: false,
                    verify_echo: false,
                    verify_submit,
                    ..Default::default()
                },
                delivery.submit_claim,
            )
            .map(|_| ())
            .map_err(|error| {
                DeliveryError::from_injection(&delivery.pty_id, error).ledger_message()
            })
        })
    }
}
