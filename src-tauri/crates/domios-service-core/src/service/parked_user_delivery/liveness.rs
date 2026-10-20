use super::*;
use crate::service::pending_delivery_payloads::user_sends::ended_launch::EndedHeadSettlement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UserClearingEvent {
    Readiness,
    Identity,
    Settlement,
}

#[derive(Debug, PartialEq, Eq)]
enum HeadProgress {
    Wait(UserClearingEvent),
    Terminalize,
}

fn head_progress(ended_launch: bool, event: UserClearingEvent) -> HeadProgress {
    if ended_launch {
        HeadProgress::Terminalize
    } else {
        HeadProgress::Wait(event)
    }
}

impl crate::service::ServiceContext {
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
        match head_progress(
            self.user_authority_is_from_ended_launch(&head.authority),
            event,
        ) {
            HeadProgress::Terminalize => {
                self.settle_ended_user_head(head)?;
                Ok(false)
            }
            HeadProgress::Wait(event) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_head_wait_requires_a_live_launch_and_a_clearing_event() {
        for event in [
            UserClearingEvent::Readiness,
            UserClearingEvent::Identity,
            UserClearingEvent::Settlement,
        ] {
            assert_eq!(head_progress(false, event), HeadProgress::Wait(event));
            assert_eq!(head_progress(true, event), HeadProgress::Terminalize);
        }
    }
}
