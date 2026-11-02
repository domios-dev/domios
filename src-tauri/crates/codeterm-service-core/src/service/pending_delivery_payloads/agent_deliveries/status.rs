use super::*;
use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};

const MAX_INSPECTION_RECEIPTS: usize = 256;

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

impl AgentDeliveryStore {
    pub(super) fn retain_inspection(
        &self,
        tab: &AgentDeliveryTab,
        record: &AgentDeliveryRecord,
        written: bool,
    ) -> Result<(), String> {
        let dir = tab.dir.join("inspection");
        owner_directory(&dir)?;
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
        let bytes = serde_json::to_vec(&receipt).map_err(|error| error.to_string())?;
        let temporary = tab.dir.join("inspection.next.json");
        write_replace(&temporary, &bytes)?;
        atomic_replace(&temporary, &dir.join(format!("{}.json", record.id)))?;
        sync_directory(&dir)?;
        let mut receipts = fs::read_dir(&dir)
            .map_err(|error| error.to_string())?
            .take(MAX_INSPECTION_RECEIPTS + 2)
            .map(|entry| {
                let path = entry.map_err(|error| error.to_string())?.path();
                let receipt: ChatSendStatus =
                    serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
                        .map_err(|_| "agent delivery inspection receipt is invalid")?;
                Ok((receipt.sequence, path))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if receipts.len() > MAX_INSPECTION_RECEIPTS + 1 {
            return Err("agent delivery inspection receipt bound exceeded".into());
        }
        receipts.sort_by_key(|(sequence, _)| *sequence);
        for (_, path) in receipts
            .iter()
            .take(receipts.len().saturating_sub(MAX_INSPECTION_RECEIPTS))
        {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        sync_directory(&dir)
    }

    pub(crate) fn inspect(&self, id: &str) -> Result<Option<ChatSendStatus>, String> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Ok(None);
        }
        for tab_id in self.persisted_tab_ids()? {
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
            let record = index.records.iter().find(|record| record.id == id);
            let mut state = record.map(|record| {
                if record.terminal_failure.is_some() {
                    ChatSendState::Failed
                } else if record.state == AgentDeliveryState::Pending {
                    ChatSendState::Pending
                } else {
                    ChatSendState::Unknown
                }
            });
            let mut sequence = record.map(|record| record.sequence);
            let error = record.and_then(|record| record.terminal_failure.clone());
            for path in [
                settled_receipt_path(&tab, id)?,
                pending_receipt_path(&tab, id)?,
            ] {
                let bytes = match read_bounded(&path, MAX_RECEIPT_BYTES) {
                    Ok(bytes) => bytes,
                    Err(_) if !path.try_exists().map_err(|error| error.to_string())? => continue,
                    Err(error) => return Err(error),
                };
                let receipt: AgentDeliveryReceipt = serde_json::from_slice(&bytes)
                    .map_err(|_| "agent delivery receipt is invalid")?;
                if receipt.id != id || receipt.tab_id != tab_id {
                    return Err("agent delivery receipt identity mismatch".into());
                }
                sequence = Some(receipt.sequence);
                state = Some(match receipt.status {
                    Some(AgentDeliveryReceiptStatus::Delivered) => ChatSendState::Sent,
                    _ => ChatSendState::Unknown,
                });
                break;
            }
            if let (Some(state), Some(sequence)) = (state, sequence) {
                return Ok(Some(status(&tab_id, id, sequence, state, error)));
            }
            let path = tab.dir.join("inspection").join(format!("{id}.json"));
            if path.try_exists().map_err(|error| error.to_string())? {
                let receipt: ChatSendStatus =
                    serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
                        .map_err(|_| "agent delivery inspection receipt is invalid")?;
                if receipt.send_id != id || receipt.tab_id != tab_id {
                    return Err("agent delivery inspection receipt identity mismatch".into());
                }
                return Ok(Some(receipt));
            }
        }
        Ok(None)
    }
}
