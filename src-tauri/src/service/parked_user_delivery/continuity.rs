use super::*;
use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};

impl super::super::ServiceContext {
    pub(crate) fn restore_durable_user_statuses(&self, tab_id: &str) -> Result<(), String> {
        if self.tab_chat_backing(tab_id).is_some() {
            return Ok(());
        }
        let store = &self.pending_delivery_payloads.user_sends;
        for receipt in store.receipts(tab_id)? {
            self.chat_send_ledger.restore_user_receipt(&receipt);
        }
        for record in store.records(tab_id)? {
            self.chat_send_ledger.restore_user_record(&record);
        }
        Ok(())
    }

    pub(crate) fn reconcile_user_send_statuses(
        &self,
        tab_id: &str,
    ) -> Result<Vec<ChatSendStatus>, String> {
        self.restore_durable_user_statuses(tab_id)?;
        self.reconcile_chat_send_evidence(tab_id);
        let statuses = self.chat_send_ledger.reconcile(tab_id);
        let delivered = statuses
            .iter()
            .filter(|status| status.state == ChatSendState::Sent)
            .map(|status| status.send_id.clone())
            .collect::<Vec<_>>();
        self.pending_delivery_payloads
            .user_sends
            .record_delivered(tab_id, &delivered)?;
        self.wake_parked_user_send(tab_id);
        Ok(statuses)
    }

    pub(super) fn held_user_identity_is_unresolved(&self, record: &UserSendRecord) -> bool {
        record.authority.observed_session_id.is_some()
            && self
                .mux
                .read()
                .get_tab(&record.authority.tab_id)
                .is_some_and(|tab| {
                    tab.agent_type.is_none()
                        || (tab.agent_type.as_deref()
                            == Some(record.authority.provider_id.as_str())
                            && tab.agent_session_id.is_none())
                })
    }

    pub(super) fn carry_held_user_send(
        &self,
        record: &UserSendRecord,
    ) -> Result<Option<UserSendRecord>, String> {
        if self
            .chat_send_ledger
            .user_payload_may_have_been_written(&record.send_id)
        {
            return Ok(None);
        }
        let Ok(authority) = self.user_send_authority(&record.authority.tab_id) else {
            return Ok(None);
        };
        let carried = self
            .pending_delivery_payloads
            .user_sends
            .carry_held_send(record, authority)?;
        if let Some(carried) = &carried {
            if carried.authority == record.authority
                && carried.owner_epoch == record.owner_epoch
                && carried.held_before_readiness == record.held_before_readiness
            {
                return Ok(Some(carried.clone()));
            }
            if carried.authority != record.authority {
                self.chat_send_ledger
                    .take_user_submit_claim(&record.send_id);
            }
            if !self.chat_send_ledger.rebind_queued_user_authority(
                &record.send_id,
                &record.authority,
                &carried.authority,
            ) {
                return Err("carried user authority has no matching held ledger admission".into());
            }
            log::debug!(
                "[chat-send] carried held send tab={} send_id={} pty={:?}",
                carried.authority.tab_id,
                carried.send_id,
                carried.authority.pty_id
            );
        }
        Ok(carried)
    }
}
