use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryRecord};
use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};

fn settle_receipt(record: &mut PendingDeliveryRecord, status: &ChatSendStatus) -> bool {
    if record.channel != PendingDeliveryChannel::InteractiveInput
        || record.send_id() != Some(status.send_id.as_str())
        || record.state().is_accepted()
    {
        return false;
    }
    let before = record.state();
    match status.state {
        ChatSendState::Sent => {
            record.mark_ingested();
            let _ = record.confirm();
        }
        ChatSendState::Written | ChatSendState::Unknown => record.mark_unconfirmed(),
        ChatSendState::Pending | ChatSendState::Failed => {}
    }
    before != record.state()
}

impl crate::service::ServiceContext {
    pub(super) fn settle_pending_delivery_receipt(&self, tab_id: &str) {
        let record = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pending_delivery.clone());
        let Some(record) = record else { return };
        let Some(status) = record
            .send_id()
            .and_then(|id| self.chat_send_ledger.status(id))
        else {
            return;
        };
        if status.tab_id != tab_id {
            return;
        }
        let changed = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .and_then(|tab| tab.pending_delivery.as_mut())
                .filter(|current| current.record_id() == record.record_id())
                .is_some_and(|current| settle_receipt(current, &status))
        };
        if !changed {
            return;
        }
        if let Err(error) = crate::service::session_persist::persist_session_after_mutation(
            self,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        ) {
            log::warn!("[chat-send] pending receipt persistence tab={tab_id} failed: {error}");
            return;
        }
        if status.state == ChatSendState::Sent {
            if let Err(error) = self.cleanup_confirmed_pending_delivery(tab_id) {
                log::warn!("[chat-send] pending receipt cleanup tab={tab_id} failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::types::PendingDeliveryState;
    use crate::service::chat_send_transaction::ChatSendLedger;

    #[test]
    fn only_a_correlated_written_receipt_settles_pending_input() {
        let ledger = ChatSendLedger::default();
        let send = ledger.try_start("own", "opaque", None).unwrap();
        let mut record =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
                .with_send_id(send.send_id.clone());
        assert!(!settle_receipt(&mut record, &send));
        let mut written = send.clone();
        written.state = ChatSendState::Written;
        let mut foreign = written.clone();
        foreign.send_id = "another-send".into();
        assert!(!settle_receipt(&mut record, &foreign));
        assert_eq!(record.state(), PendingDeliveryState::Pending);
        assert!(settle_receipt(&mut record, &written));
        assert_eq!(record.state(), PendingDeliveryState::Unconfirmed);
        assert!(!settle_receipt(&mut record, &written));
        let delivered = ledger.confirm_send(&send.send_id).unwrap();
        assert!(settle_receipt(&mut record, &delivered));
        assert_eq!(record.state(), PendingDeliveryState::AcceptedWeak);
        assert!(!settle_receipt(&mut record, &written));
        assert_eq!(record.state(), PendingDeliveryState::AcceptedWeak);
    }

    #[test]
    fn proven_zero_write_failure_keeps_pending_authority() {
        let ledger = ChatSendLedger::default();
        let send = ledger.try_start("own", "opaque", None).unwrap();
        let failed = ledger
            .finish(&send.send_id, Err("pty_unavailable: prewrite".into()))
            .unwrap();
        let mut record =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
                .with_send_id(send.send_id);
        assert!(!settle_receipt(&mut record, &failed));
        assert_eq!(record.state(), PendingDeliveryState::Pending);
    }
}
