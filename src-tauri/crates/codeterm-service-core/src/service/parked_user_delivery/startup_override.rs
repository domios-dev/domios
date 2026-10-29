use super::*;

impl crate::service::ServiceContext {
    pub(crate) fn note_user_launch_dispatched(&self, authority: &UserSendAuthority) -> bool {
        self.user_authority_matches_launch_current(authority)
            && self
                .pending_delivery_payloads
                .user_sends
                .mark_launch_dispatched(authority)
    }

    pub fn override_user_startup(&self, tab_id: &str, nonce: &str) -> Result<(), String> {
        self.require_tab_input_allowed(tab_id, None)?;
        let authority = self.user_send_authority(tab_id)?;
        if authority.binding_nonce != nonce
            || !self.user_authority_matches_launch_current(&authority)
        {
            return Err("startup override launch is no longer current".into());
        }
        let stage = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.boot_stage);
        if matches!(stage, Some(codeterm_types::pty::BootStage::SpawnFailed)) {
            return Err("startup override cannot revive a failed launch".into());
        }
        if self.user_authority_is_ready(&authority) {
            return Ok(());
        }
        if !self
            .pending_delivery_payloads
            .user_sends
            .launch_was_dispatched(&authority)
        {
            return Err(
                "startup override requires the provider launch command to be written".into(),
            );
        }
        if !self.rearm_user_input_for_authority(&authority)
            || !self
                .pending_delivery_payloads
                .user_sends
                .latch_ready_launch(&authority)
        {
            return Err("startup override has no live PTY".into());
        }
        let attempt = crate::service::launch_readiness::LaunchAttempt {
            tab_id: tab_id.into(),
            pty_id: authority
                .pty_id
                .clone()
                .ok_or("startup override has no PTY")?,
            binding_nonce: authority.binding_nonce.clone(),
        };
        self.record_provisional_launch(
            &attempt,
            codeterm_types::pty::BootStage::LaunchBindingUnconfirmed,
            "user authorized input before launch readiness",
        );
        self.wake_user_owner(tab_id, UserWake::Readiness);
        Ok(())
    }
}
