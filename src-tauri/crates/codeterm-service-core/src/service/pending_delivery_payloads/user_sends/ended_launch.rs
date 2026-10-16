use super::*;
use continuity::{retain_receipt, ReceiptState};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EndedHeadSettlement {
    Refresh,
    Settled(ReceiptState),
}

fn ended_head_receipt(record: &UserSendRecord, possibly_written: bool) -> ReceiptState {
    if record.state == UserSendState::Queued
        && record.handoff_session.is_none()
        && !possibly_written
    {
        ReceiptState::Failed
    } else {
        ReceiptState::Uncertain
    }
}

impl UserSendStore {
    pub(crate) fn settle_ended_head(
        &self,
        record: &UserSendRecord,
        possibly_written: bool,
    ) -> Result<EndedHeadSettlement, String> {
        let tab = self.tab(&record.authority.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let Some(current) = loaded
            .index
            .records
            .iter()
            .find(|item| item.send_id == record.send_id)
        else {
            return Ok(EndedHeadSettlement::Refresh);
        };
        if current.authority != record.authority || current.owner_epoch != record.owner_epoch {
            return Ok(EndedHeadSettlement::Refresh);
        }
        if tab.revoked.load(Ordering::Acquire) || tab.failed.load(Ordering::Acquire) {
            return Err("ended launch settlement requires a healthy durable owner".into());
        }
        let receipt = ended_head_receipt(current, possibly_written);
        let mut next = loaded.index.clone();
        retain_receipt(&mut next, current, receipt);
        let keep_body = receipt == ReceiptState::Uncertain && current.body_digest.is_some();
        if keep_body {
            next.records
                .iter_mut()
                .find(|item| item.send_id == record.send_id)
                .ok_or("ended user head disappeared")?
                .state = UserSendState::BodyLease;
        } else {
            next.records.retain(|item| item.send_id != record.send_id);
        }
        self.persist(&tab, &next)?;
        loaded.index = next;
        tab.unsettled
            .store(unsettled_count(&loaded.index), Ordering::Release);
        drop(data);
        if !keep_body {
            self.cleanup_dismissed_payload(record);
        }
        Ok(EndedHeadSettlement::Settled(receipt))
    }
}

#[cfg(test)]
mod tests;
