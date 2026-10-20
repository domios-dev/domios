use super::*;

const MAX_DISMISSALS: usize = 256;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DismissedBubble {
    pub(crate) id: String,
    #[serde(rename = "correlationId")]
    pub(crate) correlation_id: String,
    pub(crate) content: String,
    #[serde(rename = "sentAtMs")]
    pub(crate) sent_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) echo: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserSendDismissal {
    pub(crate) send_id: String,
    pub(crate) send: DismissedBubble,
}

impl UserSendStore {
    pub fn dismissals(&self, tab_id: &str) -> Result<Vec<UserSendDismissal>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        Ok(self.load(&tab, &mut data)?.index.dismissals.clone())
    }

    pub(crate) fn cancel_or_dismiss(
        &self,
        tab_id: &str,
        mut dismissal: UserSendDismissal,
    ) -> Result<UserSendDismissal, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        if tab.failed.load(Ordering::Acquire) {
            return Err("user dismissal owner is unavailable".into());
        }
        if let Some(existing) = loaded.index.dismissals.iter().find(|item| {
            item.send_id == dismissal.send_id
                || item.send.correlation_id == dismissal.send.correlation_id
        }) {
            return Ok(existing.clone());
        }
        if loaded.index.dismissals.len() >= MAX_DISMISSALS
            || dismissal.send.content.len() > MAX_PREPARED_BYTES
        {
            return Err("user dismissal capacity reached".into());
        }
        let mut next = loaded.index.clone();
        let matching = next
            .records
            .iter()
            .find(|record| {
                record.send_id == dismissal.send_id
                    || record.client_token.as_deref()
                        == Some(dismissal.send.correlation_id.as_str())
            })
            .cloned();
        if let Some(record) = &matching {
            dismissal.send_id = record.send_id.clone();
        }
        let cancelled = matching.filter(|record| {
            record.state == UserSendState::Queued && record.handoff_session.is_none()
        });
        if let Some(record) = &cancelled {
            next.records.retain(|item| item.send_id != record.send_id);
            continuity::retain_receipt(&mut next, record, continuity::ReceiptState::Failed);
        }
        next.dismissals.push(dismissal.clone());
        validate_index(&next, tab_id)?;
        self.persist(&tab, &next)?;
        loaded.index = next;
        tab.unsettled
            .store(unsettled_count(&loaded.index), Ordering::Release);
        drop(data);
        if let Some(record) = cancelled {
            self.cleanup_dismissed_payload(&record);
        }
        Ok(dismissal)
    }
    pub(super) fn cleanup_dismissed_payload(&self, record: &UserSendRecord) {
        let tab = self.tab(&record.authority.tab_id);
        for suffix in ["original", "prepared", "body"] {
            let result = tab.payload_path(&record.send_id, suffix).and_then(|path| {
                match fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(io_error(error)),
                }
            });
            if let Err(error) = result {
                log::debug!("[chat-send] dismissed payload cleanup failed: {error}");
            }
        }
        if let Err(error) = sync_directory(&tab.dir) {
            log::debug!("[chat-send] dismissed payload cleanup sync failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests;
