use super::*;
use codeterm_types::pty::BootStage;

fn can_settle_degraded(
    state: UserSendState,
    held_before_readiness: bool,
    handoff: bool,
    ready: bool,
    possibly_written: bool,
    stage: Option<BootStage>,
) -> bool {
    state == UserSendState::Queued
        && held_before_readiness
        && !handoff
        && !ready
        && !possibly_written
        && matches!(
            stage,
            Some(
                BootStage::SpawnFailed
                    | BootStage::LaunchDeadlineElapsed
                    | BootStage::LaunchBindingUnconfirmed
            )
        )
}

impl crate::service::ServiceContext {
    pub(super) fn settle_degraded_user_send(
        &self,
        record: &UserSendRecord,
    ) -> Result<bool, String> {
        let stage = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(&record.authority.tab_id) else {
                return Ok(false);
            };
            if !self.user_authority_matches_launch(tab, &record.authority)
                && !(record.authority.prelaunch
                    && tab.agent_type.as_deref() == Some(record.authority.provider_id.as_str())
                    && (record.authority.pty_id.is_none() || tab.pty_id == record.authority.pty_id)
                    && (tab.session_binding_nonce.as_deref()
                        == Some(record.authority.binding_nonce.as_str())
                        || tab
                            .prior_session_binding_nonces
                            .last()
                            .is_some_and(|prior| prior.nonce == record.authority.binding_nonce)))
            {
                return Ok(false);
            }
            tab.boot_stage
        };
        if !can_settle_degraded(
            record.state,
            record.held_before_readiness,
            record.handoff_session.is_some(),
            self.user_send_authority(&record.authority.tab_id)
                .is_ok_and(|current| {
                    self.pending_delivery_payloads
                        .user_sends
                        .ready_launch_is_latched(&current)
                }),
            self.chat_send_ledger
                .user_payload_may_have_been_written(&record.send_id),
            stage,
        ) {
            return Ok(false);
        }
        if !self
            .pending_delivery_payloads
            .user_sends
            .discard_queued_for_degraded_launch(record)?
        {
            return Ok(false);
        }
        log::debug!(
            "[chat-send] degraded launch settled never-written send tab={} send_id={} stage={stage:?}",
            record.authority.tab_id,
            record.send_id,
        );
        if let Some(status) = self
            .chat_send_ledger
            .expire_owned_before_write(&record.send_id, "launch_degraded_before_write")
        {
            crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
        self.local_recordings.retire_write_ticket(&record.send_id);
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
