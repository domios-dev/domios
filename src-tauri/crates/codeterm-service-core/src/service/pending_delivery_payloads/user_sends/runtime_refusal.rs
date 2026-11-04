use super::continuity::{ReceiptState, UserSendReceipt, MAX_RECEIPTS};
use super::*;
use crate::service::chat_send_transaction::ChatSendStatus;

impl UserSendStore {
    pub(crate) fn persist_rpc_refusal(
        &self,
        status: &ChatSendStatus,
        thread: &str,
        nonce: &str,
    ) -> Result<bool, String> {
        let tab = self.tab(&status.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        if tab.revoked.load(Ordering::Acquire) || tab.failed.load(Ordering::Acquire) {
            return Ok(false);
        }
        let mut next = loaded.index.clone();
        if next
            .source_claims
            .iter()
            .any(|claim| claim.send_id == status.send_id)
            || next.receipts.iter().any(|receipt| {
                receipt.send_id == status.send_id && receipt.state == ReceiptState::Delivered
            })
        {
            return Ok(false);
        }
        if let Some(record) = next
            .records
            .iter()
            .find(|record| record.send_id == status.send_id)
        {
            if record.authority.tab_id != status.tab_id
                || record.authority.binding_nonce != nonce
                || record.authority.session_id.as_deref() != Some(thread)
                || !matches!(
                    record.state,
                    UserSendState::Claimed | UserSendState::Uncertain | UserSendState::BodyLease
                )
            {
                return Ok(false);
            }
        } else if !next.receipts.iter().any(|receipt| {
            receipt.send_id == status.send_id
                && matches!(
                    receipt.state,
                    ReceiptState::Uncertain | ReceiptState::RpcRefused
                )
        }) {
            return Ok(false);
        }
        next.receipts
            .retain(|receipt| receipt.send_id != status.send_id);
        next.receipts.push(UserSendReceipt {
            send_id: status.send_id.clone(),
            tab_id: status.tab_id.clone(),
            client_token: status.correlation_id.clone(),
            ledger_sequence: status.sequence,
            state: ReceiptState::RpcRefused,
        });
        if next.receipts.len() > MAX_RECEIPTS {
            next.receipts.remove(0);
        }
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
