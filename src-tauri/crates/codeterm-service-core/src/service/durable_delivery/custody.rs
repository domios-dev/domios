use super::super::pending_delivery_payloads::agent_deliveries::AgentDeliveryState;
use super::*;

impl ServiceContext {
    /// Commits authenticated session custody before asynchronous binding follow-up.
    pub fn commit_session_binding_custody(
        &self,
        target: &str,
        bind: impl FnOnce() -> Result<
            super::super::hook_session_binding::SessionStartClaim,
            super::super::hook_session_binding::SessionBindError,
        >,
    ) -> Result<
        super::super::hook_session_binding::SessionStartClaim,
        super::super::hook_session_binding::SessionBindError,
    > {
        let authority = self.pty_ownership.acquire(target);
        let claim = bind()?;
        if claim.nonce_verified {
            if let Err(error) = self.bind_authenticated_durable_deliveries_claimed(&authority) {
                log::error!(
                    "[agent-delivery] authenticated commit custody failed tab={target}: {error}"
                );
            }
        }
        Ok(claim)
    }

    pub(crate) fn bind_authenticated_durable_deliveries(&self, target: &str) -> Result<(), String> {
        let claim = self.pty_ownership.acquire(target);
        self.bind_authenticated_durable_deliveries_claimed(&claim)
    }

    fn bind_authenticated_durable_deliveries_claimed(
        &self,
        claim: &super::super::pty_ownership::TabPtyClaim,
    ) -> Result<(), String> {
        let target = claim.tab_id();
        let observed = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(target) else {
                return Ok(());
            };
            if tab.restored_from_snapshot || !tab.launch_session_authenticated() {
                return Ok(());
            }
            let (Some(provider), Some(session), Some(pty), Some(nonce)) = (
                tab.agent_type.clone(),
                tab.agent_session_id.clone(),
                tab.pty_id.clone(),
                tab.session_binding_nonce.clone(),
            ) else {
                return Ok(());
            };
            (
                provider,
                SavedLaunch {
                    pty,
                    session,
                    nonce,
                },
            )
        };
        let owner = &self.pending_delivery_payloads.agent_deliveries;
        let result: Result<(), String> = (|| {
            for record in owner.records(target)? {
                if record.state != AgentDeliveryState::Pending
                    || record.saved_launch.is_some()
                    || record.terminal_failure.is_some()
                {
                    continue;
                }
                let Some(saved) = record
                    .durable_delivery
                    .as_ref()
                    .and_then(|delivery| delivery.saved_target.as_ref())
                else {
                    continue;
                };
                if let Some(cause) = self
                    .durable_runtime()
                    .binding_failures
                    .lock()
                    .get(target)
                    .cloned()
                {
                    owner.mark_terminal_failure(
                        target,
                        &record.id,
                        "launch_authority_unavailable",
                        Some(&cause),
                    )?;
                    continue;
                }
                if saved.tab == target
                    && saved.provider == observed.0
                    && saved.session == observed.1.session
                {
                    owner.bind_saved_launch(target, &record.id, observed.1.clone())?;
                } else {
                    let reason = if saved.provider != observed.0 {
                        "saved_provider_changed"
                    } else {
                        "saved_session_changed"
                    };
                    owner.mark_terminal_failure(target, &record.id, reason, None)?;
                }
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.durable_runtime()
                .binding_failures
                .lock()
                .insert(target.into(), error.clone());
        }
        result
    }

    pub(crate) fn settle_closed_durable_deliveries(&self, target: &str) -> Result<(), String> {
        let _claim = self.pty_ownership.acquire(target);
        let owner = &self.pending_delivery_payloads.agent_deliveries;
        for record in owner.records(target)? {
            if record.state != AgentDeliveryState::Pending || record.durable_delivery.is_none() {
                continue;
            }
            let payload: AgentDeliveryPayload =
                serde_json::from_str(&owner.payload(target, &record)?)
                    .map_err(|error| error.to_string())?;
            self.fail_durable_delivery(target, owner, &record, &payload, "recipient_gone")?;
        }
        Ok(())
    }
}
