use super::*;

const MAX_RECEIPTS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReceiptState {
    Failed,
    Uncertain,
    Delivered,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct UserSendReceipt {
    pub(crate) send_id: String,
    pub(crate) tab_id: String,
    pub(crate) client_token: Option<String>,
    pub(crate) ledger_sequence: u64,
    pub(crate) state: ReceiptState,
}

pub(crate) fn can_carry_held_send(record: &UserSendRecord, current: &UserSendAuthority) -> bool {
    record.state == UserSendState::Queued
        && record.handoff_session.is_none()
        && record.authority.tab_id == current.tab_id
        && record.authority.provider_id == current.provider_id
        && record
            .authority
            .observed_session_id
            .as_ref()
            .is_some_and(|session| current.observed_session_id.as_ref() == Some(session))
}

pub(crate) fn known_session_changed(previous: Option<&str>, current: Option<&str>) -> bool {
    matches!((previous, current), (Some(previous), Some(current)) if previous != current)
}

pub(super) fn retain_receipt(index: &mut UserIndex, record: &UserSendRecord, state: ReceiptState) {
    index
        .receipts
        .retain(|receipt| receipt.send_id != record.send_id);
    index.receipts.push(UserSendReceipt {
        send_id: record.send_id.clone(),
        tab_id: record.authority.tab_id.clone(),
        client_token: record.client_token.clone(),
        ledger_sequence: record.ledger_sequence,
        state,
    });
    if index.receipts.len() > MAX_RECEIPTS {
        index.receipts.drain(..index.receipts.len() - MAX_RECEIPTS);
    }
}

pub(super) fn validate_receipts(index: &UserIndex, tab_id: &str) -> Result<(), String> {
    let mut ids = HashSet::new();
    if index.receipts.len() > MAX_RECEIPTS
        || index.receipts.iter().any(|receipt| {
            receipt.tab_id != tab_id
                || uuid::Uuid::parse_str(&receipt.send_id).is_err()
                || !ids.insert(&receipt.send_id)
        })
    {
        return Err("user settlement receipt identity is invalid".into());
    }
    Ok(())
}

fn settle_fresh_launch(index: &mut UserIndex, possibly_written: &[String]) -> Vec<UserSendRecord> {
    let ended = index
        .records
        .iter()
        .filter(|record| {
            record.state == UserSendState::Queued
                && record.handoff_session.is_none()
                && !possibly_written.contains(&record.send_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    for record in &ended {
        index.records.retain(|item| item.send_id != record.send_id);
        retain_receipt(index, record, ReceiptState::Failed);
    }
    ended
}

impl UserSendStore {
    pub(crate) fn end_held_for_fresh_restart(
        &self,
        tab_id: &str,
        binding_nonce: Option<String>,
        possibly_written: &[String],
    ) -> Result<Vec<UserSendRecord>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        if tab.revoked.load(Ordering::Acquire) || tab.failed.load(Ordering::Acquire) {
            return Err("fresh restart user owner is held or revoked".into());
        }
        let mut next = loaded.index.clone();
        let ended = settle_fresh_launch(&mut next, possibly_written);
        self.persist(&tab, &next)?;
        loaded.index = next;
        *tab.ended_launch_nonce.lock() = binding_nonce;
        tab.unsettled
            .store(unsettled_count(&loaded.index), Ordering::Release);
        drop(data);
        for record in &ended {
            for suffix in ["original", "prepared", "body"] {
                let path = tab.payload_path(&record.send_id, suffix)?;
                if let Err(error) = fs::remove_file(path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        log::warn!("[chat-send] fresh restart payload cleanup failed: {error}");
                    }
                }
            }
        }
        if let Err(error) = sync_directory(&tab.dir) {
            log::warn!("[chat-send] fresh restart payload cleanup sync failed: {error}");
        }
        Ok(ended)
    }

    pub(crate) fn carry_held_send(
        &self,
        record: &UserSendRecord,
        authority: UserSendAuthority,
    ) -> Result<Option<UserSendRecord>, String> {
        if !can_carry_held_send(record, &authority) {
            return Ok(None);
        }
        let tab = self.tab(&authority.tab_id);
        if tab.revoked.load(Ordering::Acquire) || tab.failed.load(Ordering::Acquire) {
            return Ok(None);
        }
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        let Some(current) = next
            .records
            .iter_mut()
            .find(|item| item.send_id == record.send_id)
        else {
            return Ok(None);
        };
        if current.authority != record.authority
            || current.owner_epoch != record.owner_epoch
            || !can_carry_held_send(current, &authority)
        {
            return Ok(None);
        }
        if current.owner_epoch == self.epoch && current.authority == authority {
            return Ok(Some(current.clone()));
        }
        if current.owner_epoch != self.epoch {
            current.reservation = 0;
        }
        current.owner_epoch = self.epoch.clone();
        current.authority = authority;
        current.held_before_readiness = true;
        let carried = current.clone();
        self.persist(&tab, &next)?;
        loaded.index = next;
        Ok(Some(carried))
    }

    pub(crate) fn receipts(&self, tab_id: &str) -> Result<Vec<UserSendReceipt>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        Ok(self.load(&tab, &mut data)?.index.receipts.clone())
    }

    pub(crate) fn record_delivered(&self, tab_id: &str, send_ids: &[String]) -> Result<(), String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        let mut changed = false;
        for receipt in &mut next.receipts {
            if receipt.state == ReceiptState::Uncertain && send_ids.contains(&receipt.send_id) {
                receipt.state = ReceiptState::Delivered;
                changed = true;
            }
        }
        if changed {
            self.persist(&tab, &next)?;
            loaded.index = next;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
