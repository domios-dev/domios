use super::*;
use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};

const MAX_INSPECTION_RECEIPTS: usize = 256;
const MAX_LOCATOR_BYTES: u64 = 4096;

fn status(
    tab_id: &str,
    id: &str,
    sequence: u64,
    state: ChatSendState,
    error: Option<String>,
) -> ChatSendStatus {
    ChatSendStatus {
        send_id: id.into(),
        tab_id: tab_id.into(),
        state,
        error,
        ledger_id: "agent-delivery-outbox".into(),
        sequence,
        retained_from: sequence,
        correlation_id: None,
        matched_row: None,
        pending_reason: None,
        system_send: None,
    }
}

fn publish_inspection(tab: &AgentDeliveryTab, receipt: &ChatSendStatus) -> Result<(), String> {
    let dir = tab.dir.join("inspection");
    owner_directory(&dir)?;
    let bytes = serde_json::to_vec(receipt).map_err(|error| error.to_string())?;
    let temporary = tab.dir.join("inspection.next.json");
    write_replace(&temporary, &bytes)?;
    atomic_replace(&temporary, &dir.join(format!("{}.json", receipt.send_id)))?;
    sync_directory(&dir)
}

impl AgentDeliveryStore {
    fn locator_path(&self, id: &str) -> PathBuf {
        self.root.join("locators").join(format!("{id}.json"))
    }

    pub(super) fn write_locator(&self, tab_id: &str, id: &str) -> Result<(), String> {
        let dir = self.root.join("locators");
        owner_directory(&dir)?;
        let bytes = serde_json::to_vec(&(id, tab_id)).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_LOCATOR_BYTES {
            return Err("agent delivery locator exceeds its bound".into());
        }
        write_new(&self.locator_path(id), &bytes)?;
        sync_directory(&dir)
    }

    pub(super) fn remove_locator(&self, id: &str) {
        if let Err(error) = remove_if_present(&self.locator_path(id)) {
            log::debug!("[agent-delivery] inspection locator cleanup failed: {error}");
        }
    }

    fn prune_inspection(&self, tab: &AgentDeliveryTab) {
        let entries = match fs::read_dir(tab.dir.join("inspection")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                log::debug!("[agent-delivery] inspection retention unavailable: {error}");
                return;
            }
        };
        let mut retained = std::collections::BTreeMap::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    log::debug!("[agent-delivery] inspection receipt entry unavailable: {error}");
                    continue;
                }
            };
            let path = entry.path();
            let id = entry
                .file_name()
                .to_string_lossy()
                .trim_end_matches(".json")
                .to_string();
            if uuid::Uuid::parse_str(&id).is_err() {
                continue;
            }
            let receipt = read_bounded(&path, MAX_INDEX_BYTES).and_then(|bytes| {
                serde_json::from_slice::<ChatSendStatus>(&bytes).map_err(|error| error.to_string())
            });
            let receipt = match receipt {
                Ok(receipt) if receipt.send_id == id && receipt.tab_id == tab.tab_id => receipt,
                _ => {
                    log::debug!(
                        "[agent-delivery] discarding invalid or unreadable inspection receipt"
                    );
                    self.discard_inspection(&path, &id);
                    continue;
                }
            };
            retained.insert((receipt.sequence, id), path);
            if retained.len() > MAX_INSPECTION_RECEIPTS {
                if let Some(((_, id), path)) = retained.pop_first() {
                    self.discard_inspection(&path, &id);
                }
            }
        }
    }

    fn discard_inspection(&self, path: &Path, id: &str) {
        match remove_if_present(path) {
            Ok(()) => self.remove_locator(id),
            Err(error) => {
                log::debug!("[agent-delivery] inspection receipt cleanup failed: {error}")
            }
        }
    }

    pub(super) fn retain_inspection(
        &self,
        tab: &AgentDeliveryTab,
        record: &AgentDeliveryRecord,
        written: bool,
    ) {
        self.prune_inspection(tab);
        let receipt = status(
            &tab.tab_id,
            &record.id,
            record.sequence,
            if written {
                ChatSendState::Unknown
            } else {
                ChatSendState::Failed
            },
            (!written).then(|| {
                record
                    .terminal_failure
                    .clone()
                    .unwrap_or_else(|| "launch_ended_before_write".into())
            }),
        );
        if let Err(error) = publish_inspection(tab, &receipt) {
            log::debug!("[agent-delivery] inspection receipt publication failed: {error}");
        }
        self.prune_inspection(tab);
    }

    pub(crate) fn inspect(&self, id: &str) -> Result<Option<ChatSendStatus>, String> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Ok(None);
        }
        let path = self.locator_path(id);
        if !path.try_exists().map_err(|error| error.to_string())? {
            return Ok(None);
        }
        let (stored_id, tab_id): (String, String) =
            serde_json::from_slice(&read_bounded(&path, MAX_LOCATOR_BYTES)?)
                .map_err(|_| "agent delivery locator is invalid")?;
        if stored_id != id {
            return Err("agent delivery locator identity mismatch".into());
        }
        let tab = self.tab(&tab_id);
        let loaded = tab.index.lock();
        let index = match loaded.as_ref() {
            Some(index) => index.clone(),
            None => serde_json::from_slice::<AgentDeliveryIndex>(&read_bounded(
                &tab.dir.join("index.json"),
                MAX_INDEX_BYTES,
            )?)
            .map_err(|_| "agent delivery index is invalid")?,
        };
        self.validate(&index)?;
        if !index.tab_id.is_empty() && index.tab_id != tab_id {
            return Err("agent delivery tab identity mismatch".into());
        }
        for path in [
            settled_receipt_path(&tab, id)?,
            pending_receipt_path(&tab, id)?,
        ] {
            if !path.try_exists().map_err(|error| error.to_string())? {
                continue;
            }
            let receipt: AgentDeliveryReceipt =
                serde_json::from_slice(&read_bounded(&path, MAX_RECEIPT_BYTES)?)
                    .map_err(|_| "agent delivery receipt is invalid")?;
            if receipt.id != id || receipt.tab_id != tab_id {
                return Err("agent delivery receipt identity mismatch".into());
            }
            return Ok(Some(status(
                &tab_id,
                id,
                receipt.sequence,
                match receipt.status {
                    Some(AgentDeliveryReceiptStatus::Delivered) => ChatSendState::Sent,
                    _ => ChatSendState::Unknown,
                },
                None,
            )));
        }
        if let Some(record) = index.records.iter().find(|record| record.id == id) {
            let state = if record.terminal_failure.is_some() {
                ChatSendState::Failed
            } else if record.state == AgentDeliveryState::Pending {
                ChatSendState::Pending
            } else {
                ChatSendState::Unknown
            };
            return Ok(Some(status(
                &tab_id,
                id,
                record.sequence,
                state,
                record.terminal_failure.clone(),
            )));
        }
        let path = tab.dir.join("inspection").join(format!("{id}.json"));
        if !path.try_exists().map_err(|error| error.to_string())? {
            return Ok(None);
        }
        let receipt: ChatSendStatus =
            serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
                .map_err(|_| "agent delivery inspection receipt is invalid")?;
        if receipt.send_id != id || receipt.tab_id != tab_id {
            return Err("agent delivery inspection receipt identity mismatch".into());
        }
        Ok(Some(receipt))
    }
}

#[cfg(test)]
mod tests;
