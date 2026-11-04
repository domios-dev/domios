use super::*;
use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

const MAX_INSPECTION_RECEIPTS: usize = 256;

pub(crate) fn send_id(tab_id: &str, id: &str) -> String {
    format!("agent-pty:{}:{id}", URL_SAFE_NO_PAD.encode(tab_id))
}

pub(crate) fn parse_send_id(value: &str) -> Option<(String, String)> {
    if value.len() > 512 {
        return None;
    }
    let (tab, id) = value.strip_prefix("agent-pty:")?.split_once(':')?;
    uuid::Uuid::parse_str(id).ok()?;
    let tab_id = String::from_utf8(URL_SAFE_NO_PAD.decode(tab).ok()?).ok()?;
    if tab_id.is_empty() || tab_id.len() > 256 || send_id(&tab_id, id) != value {
        return None;
    }
    Some((tab_id, id.into()))
}

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

fn publish_inspection(
    tab: &AgentDeliveryTab,
    receipt: &ChatSendStatus,
    bytes_written: bool,
) -> Result<(), String> {
    let dir = tab.dir.join("inspection");
    owner_directory(&dir)?;
    let mut document = serde_json::to_value(receipt).map_err(|error| error.to_string())?;
    document["bytes_written"] = serde_json::json!(bytes_written);
    let bytes = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
    let temporary = tab.dir.join("inspection.next.json");
    write_replace(&temporary, &bytes)?;
    atomic_replace(&temporary, &dir.join(format!("{}.json", receipt.send_id)))?;
    sync_directory(&dir)
}

impl AgentDeliveryStore {
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
                    self.discard_inspection(&path);
                    continue;
                }
            };
            retained.insert((receipt.sequence, id), path);
            if retained.len() > MAX_INSPECTION_RECEIPTS {
                if let Some(((_, _id), path)) = retained.pop_first() {
                    self.discard_inspection(&path);
                }
            }
        }
    }

    fn discard_inspection(&self, path: &Path) {
        match remove_if_present(path) {
            Ok(()) => {}
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
    ) -> Result<(), String> {
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
        if let Err(error) = publish_inspection(tab, &receipt, written) {
            if !written {
                return Err(error);
            }
            log::debug!("[agent-delivery] inspection receipt publication failed: {error}");
        }
        self.prune_inspection(tab);
        Ok(())
    }

    pub(crate) fn inspected_payload_written(&self, public_id: &str) -> Result<bool, String> {
        let Some((tab_id, id)) = parse_send_id(public_id) else {
            return Ok(false);
        };
        let path = self
            .root
            .join(digest(tab_id.as_bytes()))
            .join("inspection")
            .join(format!("{id}.json"));
        if !path.try_exists().map_err(|error| error.to_string())? {
            return Ok(false);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
                .map_err(|error| error.to_string())?;
        if document["tab_id"] != tab_id || document["send_id"] != id {
            return Err("agent delivery inspection receipt identity mismatch".into());
        }
        Ok(document["bytes_written"] == true)
    }

    pub(crate) fn inspect(&self, public_id: &str) -> Result<Option<ChatSendStatus>, String> {
        let Some((tab_id, id)) = parse_send_id(public_id) else {
            return Ok(None);
        };
        Ok(self.inspect_owned(&tab_id, &id)?.map(|mut status| {
            status.send_id = public_id.into();
            status
        }))
    }

    fn inspect_owned(&self, tab_id: &str, id: &str) -> Result<Option<ChatSendStatus>, String> {
        let path = self.root.join(digest(tab_id.as_bytes())).join("index.json");
        if !path.try_exists().map_err(|error| error.to_string())? {
            return Ok(None);
        }
        let tab = self.tab(tab_id);
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
                tab_id,
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
            let mut receipt = status(
                tab_id,
                id,
                record.sequence,
                state,
                record.terminal_failure.clone(),
            );
            if receipt.state == ChatSendState::Pending {
                receipt.pending_reason = Some(format!(
                    "{} {} Cancel before write with domios tab send-cancel {public_id}.",
                    record.pending_reason.as_deref().unwrap_or("Waiting for exact own-session initial readiness; binding the current recipient launch releases queued sends in order."),
                    record.ready_deadline_ms.map(|deadline| format!("Refuses without writing at Unix millisecond {deadline} if still held.")).unwrap_or_else(|| "The durable delivery TTL bounds this hold.".into()),
                    public_id = send_id(tab_id, id),
                ));
            }
            return Ok(Some(receipt));
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
