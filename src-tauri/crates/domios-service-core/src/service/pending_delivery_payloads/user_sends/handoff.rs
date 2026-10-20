use super::*;

impl UserSendStore {
    pub(crate) fn finish_handoff(
        &self,
        record: &UserSendRecord,
        proven_zero_write: bool,
    ) -> Result<(), String> {
        let tab = self.tab(&record.authority.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        let current = next
            .records
            .iter()
            .find(|item| item.send_id == record.send_id)
            .ok_or("user claim disappeared")?;
        if current.state != UserSendState::Claimed
            || current.authority != record.authority
            || current.owner_epoch != self.epoch
            || record.owner_epoch != self.epoch
        {
            return Err("user claim is not active in this owner epoch".into());
        }
        if proven_zero_write
            && next
                .dismissals
                .iter()
                .any(|item| item.send_id == record.send_id)
        {
            next.records.retain(|item| item.send_id != record.send_id);
            continuity::retain_receipt(&mut next, record, continuity::ReceiptState::Failed);
            self.persist(&tab, &next)?;
            loaded.index = next;
            tab.unsettled
                .store(unsettled_count(&loaded.index), Ordering::Release);
            drop(data);
            self.cleanup_dismissed_payload(record);
            return Ok(());
        }
        let current = next
            .records
            .iter_mut()
            .find(|item| item.send_id == record.send_id)
            .ok_or("user claim disappeared")?;
        current.state = if proven_zero_write {
            UserSendState::Queued
        } else {
            UserSendState::Uncertain
        };
        if proven_zero_write {
            current.handoff_session = None;
        }
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(())
    }
}
