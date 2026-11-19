use super::*;
use crate::service::pending_delivery_payloads::user_sends::ended_launch::EndedHeadSettlement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UserClearingEvent {
    Readiness,
    Identity,
    Settlement,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UserHeadLaunch {
    Current,
    Wait(&'static str),
    Carry,
    Ended,
    RecoveryEnded,
}

impl crate::service::ServiceContext {
    pub(super) fn user_head_launch(&self, head: &UserSendRecord) -> UserHeadLaunch {
        use codeterm_types::pty::BootStage;
        let authority = &head.authority;
        let never_written = head.state == UserSendState::Queued
            && head.handoff_session.is_none()
            && !self
                .chat_send_ledger
                .user_payload_may_have_been_written(&head.send_id)
            && !self.local_recordings.has_write_ticket(&head.send_id);
        let mux = self.mux.read();
        let Some(tab) = mux.get_tab(&authority.tab_id) else {
            return UserHeadLaunch::Current;
        };
        let recovery = tab
            .restorable_launch
            .exited_chat_binding
            .as_ref()
            .filter(|exit| {
                exit.provider_id == authority.provider_id
                    && authority.observed_session_id.as_ref() == Some(&exit.session_id)
            });
        let detached =
            authority.pty_id.is_none() && !authority.prelaunch && authority.session_id.is_some();
        if detached && recovery.is_none_or(|exit| exit.nonce != authority.binding_nonce) {
            return UserHeadLaunch::RecoveryEnded;
        }
        let ended = self.user_authority_is_from_ended_launch(tab, authority);
        if let Some(exit) = recovery.filter(|_| never_written) {
            if tab.exited_chat_recipient().is_some() {
                return UserHeadLaunch::Wait("awaiting_agent_restart");
            }
            if tab.exited_chat_resume_target().is_some()
                && tab.boot_stage == Some(BootStage::SpawnFailed)
            {
                return UserHeadLaunch::RecoveryEnded;
            }
            let reserving_resume = tab.boot_stage == Some(BootStage::PtyStarting)
                && tab.agent_type.as_ref() == Some(&exit.provider_id)
                && tab
                    .agent_session_id
                    .as_ref()
                    .is_none_or(|session| session == &exit.session_id)
                && !tab.launch_session_authenticated();
            if reserving_resume || tab.resuming_exited_chat_recipient().is_some() {
                return UserHeadLaunch::Wait("awaiting_agent_ready");
            }
            if ended
                && tab.launch_session_authenticated()
                && tab.agent_type.as_ref() == Some(&exit.provider_id)
                && tab.agent_session_id.as_ref() == Some(&exit.session_id)
            {
                return UserHeadLaunch::Carry;
            }
        }
        if ended {
            UserHeadLaunch::Ended
        } else {
            UserHeadLaunch::Current
        }
    }

    pub(super) fn settle_ended_user_head(&self, head: &UserSendRecord) -> Result<(), String> {
        let store = &self.pending_delivery_payloads.user_sends;
        let settlement = store.settle_ended_head(
            head,
            self.chat_send_ledger
                .user_payload_may_have_been_written(&head.send_id),
        )?;
        if matches!(settlement, EndedHeadSettlement::Settled(_)) {
            self.restore_durable_user_statuses(&head.authority.tab_id)?;
            if let Some(status) = self.chat_send_ledger.status(&head.send_id) {
                self.ui.emit(
                    "chat-send-status",
                    serde_json::to_value(&status).unwrap_or_default(),
                );
            }
            self.local_recordings.retire_write_ticket(&head.send_id);
            log::debug!(
                "[chat-send] ended head settled tab={} send_id={} outcome={settlement:?}",
                head.authority.tab_id,
                head.send_id
            );
        }
        Ok(())
    }

    pub(super) fn hold_live_user_head(
        &self,
        head: &UserSendRecord,
        reason: &str,
        event: UserClearingEvent,
    ) -> Result<bool, String> {
        match self.user_head_launch(head) {
            UserHeadLaunch::Ended | UserHeadLaunch::RecoveryEnded => {
                self.settle_ended_user_head(head)?;
                Ok(false)
            }
            UserHeadLaunch::Carry => Ok(false),
            launch => {
                let reason = match launch {
                    UserHeadLaunch::Wait(reason) => reason,
                    _ => reason,
                };
                log::debug!(
                    "[chat-send] head wait send_id={} reason={reason} clears_on={event:?}",
                    head.send_id
                );
                self.pending_user_send(head, reason);
                Ok(true)
            }
        }
    }
}
