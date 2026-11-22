use super::*;
use crate::ports::host::UiEvents;
use crate::service::chat_send_transaction::ChatSendState;
use codeterm_wire::envelope::{parse_envelope_spans, MessageSegment};

const MAX_APPEND_BYTES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedSendKey {
    pub send_id: String,
    pub correlation_id: String,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedSendEdit {
    #[serde(flatten)]
    pub key: QueuedSendKey,
    pub appended: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct UserSendEdit {
    pub(super) view: QueuedSendEdit,
    pub(super) prepared: String,
}

#[derive(Serialize)]
pub struct QueuedSendItem {
    #[serde(flatten)]
    key: QueuedSendKey,
    preview: String,
}

#[derive(Serialize)]
pub struct ComposerSendQueue {
    items: Vec<QueuedSendItem>,
    claimed: Vec<String>,
    edits: Vec<QueuedSendEdit>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum QueuedSendAction {
    Append {
        #[serde(flatten)]
        key: QueuedSendKey,
        text: String,
    },
    Cancel {
        #[serde(flatten)]
        key: QueuedSendKey,
    },
}

#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum QueuedSendOutcome {
    Appended { edit: QueuedSendEdit },
    Cancelled,
    Unavailable,
}

impl UserSendRecord {
    pub(super) fn valid_queue_edit(&self) -> bool {
        self.edit.as_ref().is_none_or(|edit| {
            self.composer_pipeline.is_some()
                && self.queue_key().as_ref() == Some(&edit.view.key)
                && edit.view.key.version > 0
                && edit.view.appended.starts_with("\n\n")
                && edit.prepared.starts_with("\n\n")
                && edit.view.appended.len() <= MAX_APPEND_BYTES
                && edit.prepared.len() <= MAX_APPEND_BYTES
        })
    }

    fn editable(&self) -> bool {
        self.composer_pipeline.is_some()
            && self.state == UserSendState::Queued
            && self.handoff_session.is_none()
            && !self.queue_claimed
    }

    pub(super) fn queue_version(&self) -> u32 {
        self.edit.as_ref().map_or(0, |edit| edit.view.key.version)
    }

    fn queue_key(&self) -> Option<QueuedSendKey> {
        Some(QueuedSendKey {
            send_id: self.send_id.clone(),
            correlation_id: self.client_token.clone()?,
            version: self.queue_version(),
        })
    }

    fn matches_edit(&self, key: &QueuedSendKey) -> bool {
        self.editable()
            && self.send_id == key.send_id
            && self.client_token.as_deref() == Some(key.correlation_id.as_str())
            && self.queue_version() == key.version
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn with_original_append(&self, mut original: String) -> String {
        if let Some(edit) = &self.edit {
            original.push_str(&edit.view.appended);
        }
        original
    }

    pub(super) fn with_prepared_append(&self, mut prepared: String) -> String {
        if let Some(edit) = &self.edit {
            prepared.push_str(&edit.prepared);
        }
        prepared
    }
}

impl UserSendStore {
    pub(crate) fn bind_queue_events(&self, events: Arc<dyn UiEvents>) {
        self.queue_events.get_or_init(|| events);
    }

    pub(super) fn emit_queue_change(&self, tab_id: &str) {
        if let Some(ui) = self.queue_events.get() {
            ui.emit(
                "chat-send-queue-changed",
                serde_json::json!({ "tab_id": tab_id }),
            );
        }
    }

    fn composer_queue(&self, tab_id: &str) -> Result<ComposerSendQueue, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        tab.check_available()?;
        let items = loaded
            .index
            .records
            .iter()
            .filter(|record| record.editable())
            .filter_map(|record| {
                Some(QueuedSendItem {
                    key: record.queue_key()?,
                    preview: record.composer_preview.clone(),
                })
            })
            .collect();
        let claimed = loaded
            .index
            .records
            .iter()
            .filter(|record| {
                record.client_token.is_some() && record.state == UserSendState::Claimed
            })
            .map(|record| record.send_id.clone())
            .collect();
        let mut edits = loaded
            .index
            .receipts
            .iter()
            .filter_map(|receipt| receipt.queue_edit.clone())
            .map(|edit| (edit.key.send_id.clone(), edit))
            .collect::<HashMap<_, _>>();
        for edit in loaded
            .index
            .records
            .iter()
            .filter_map(|record| record.edit.as_ref())
        {
            edits.insert(edit.view.key.send_id.clone(), edit.view.clone());
        }
        Ok(ComposerSendQueue {
            items,
            claimed,
            edits: edits.into_values().collect(),
        })
    }

    fn queue_edit_target(
        &self,
        tab_id: &str,
        key: &QueuedSendKey,
    ) -> Result<Option<UserSendRecord>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        tab.check_available()?;
        Ok(loaded
            .index
            .records
            .iter()
            .find(|record| record.matches_edit(key))
            .cloned())
    }

    fn append_queued(
        &self,
        record: &UserSendRecord,
        edit: UserSendEdit,
        follow_up: Option<String>,
    ) -> Result<bool, String> {
        let tab = self.tab(&record.authority.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        tab.check_available()?;
        let Some(key) = record.queue_key() else {
            return Ok(false);
        };
        let mut next = loaded.index.clone();
        let Some(current) = next.records.iter_mut().find(|current| {
            current.matches_edit(&key)
                && current.authority == record.authority
                && current.owner_epoch == record.owner_epoch
        }) else {
            return Ok(false);
        };
        current.edit = Some(edit);
        current.typed_follow_up = follow_up;
        validate_index(&next, &record.authority.tab_id)?;
        self.persist(&tab, &next)?;
        loaded.index = next;
        drop(data);
        self.emit_queue_change(&record.authority.tab_id);
        Ok(true)
    }

    fn cancel_queued(&self, tab_id: &str, key: &QueuedSendKey) -> Result<bool, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        tab.check_available()?;
        let Some(record) = loaded
            .index
            .records
            .iter()
            .find(|record| record.matches_edit(key))
            .cloned()
        else {
            return Ok(loaded.index.receipts.iter().any(|receipt| {
                receipt.send_id == key.send_id
                    && receipt.client_token.as_deref() == Some(key.correlation_id.as_str())
                    && receipt.state == continuity::ReceiptState::Cancelled
                    && receipt
                        .queue_edit
                        .as_ref()
                        .map_or(0, |edit| edit.key.version)
                        == key.version
            }));
        };
        let mut next = loaded.index.clone();
        next.records
            .retain(|current| current.send_id != record.send_id);
        continuity::retain_receipt(&mut next, &record, continuity::ReceiptState::Cancelled);
        validate_index(&next, tab_id)?;
        self.persist(&tab, &next)?;
        loaded.index = next;
        tab.unsettled
            .store(unsettled_count(&loaded.index), Ordering::Release);
        drop(data);
        self.cleanup_dismissed_payload(&record);
        self.emit_queue_change(tab_id);
        Ok(true)
    }
}

pub(super) fn composer_preview(original: &str) -> String {
    let mut preview = String::new();
    for segment in parse_envelope_spans(original) {
        let text = match segment.segment {
            MessageSegment::Text { text } => text,
            MessageSegment::Attachment { kind, .. } => format!("[{kind}]"),
            _ => continue,
        };
        preview.extend(
            text.chars()
                .take(160usize.saturating_sub(preview.chars().count())),
        );
        if preview.chars().count() >= 160 {
            break;
        }
    }
    preview
}

fn plain_suffix(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.len() > MAX_APPEND_BYTES {
        return Err("Queued additions require 1 to 2048 bytes of text.".into());
    }
    if parse_envelope_spans(text)
        .iter()
        .any(|part| !matches!(part.segment, MessageSegment::Text { .. }))
    {
        return Err("Queued additions support plain text only.".into());
    }
    Ok(())
}

fn append_suffix(previous: &str, next: &str) -> Result<String, String> {
    let text = format!("{previous}\n\n{next}");
    plain_suffix(&text)?;
    Ok(text)
}

impl crate::service::ServiceContext {
    pub(crate) fn user_send_follow_up(&self, provider_id: &str, text: &str) -> Option<String> {
        self.agent_registry
            .with_provider(provider_id, |provider| {
                if provider.agent_runtime().is_some() {
                    return None;
                }
                provider
                    .paste_capability()
                    .and_then(|capability| capability.typed_follow_up_for(text).map(str::to_string))
            })
            .flatten()
    }

    pub fn composer_send_queue(&self, tab_id: &str) -> Result<ComposerSendQueue, String> {
        self.bind_user_send_owner();
        self.pending_delivery_payloads
            .user_sends
            .composer_queue(tab_id)
    }

    pub fn change_composer_send_queue(
        &self,
        tab_id: &str,
        action: QueuedSendAction,
    ) -> Result<QueuedSendOutcome, String> {
        self.require_tab_input_allowed(tab_id, None)?;
        self.bind_user_send_owner();
        let store = &self.pending_delivery_payloads.user_sends;
        match action {
            QueuedSendAction::Cancel { key } => {
                if !store.cancel_queued(tab_id, &key)? {
                    return Ok(QueuedSendOutcome::Unavailable);
                }
                drop(self.chat_send_ledger.take_user_submit_claim(&key.send_id));
                if let Some(status) = self
                    .chat_send_ledger
                    .expire_owned_before_write(&key.send_id, "user_cancelled_queued_send")
                {
                    self.ui.emit(
                        "chat-send-status",
                        serde_json::to_value(status).unwrap_or_default(),
                    );
                }
                self.wake_parked_user_send(tab_id);
                Ok(QueuedSendOutcome::Cancelled)
            }
            QueuedSendAction::Append { key, text } => {
                let Some(record) = store.queue_edit_target(tab_id, &key)? else {
                    return Ok(QueuedSendOutcome::Unavailable);
                };
                if self
                    .chat_send_ledger
                    .status(&key.send_id)
                    .is_some_and(|status| status.state != ChatSendState::Pending)
                {
                    return Ok(QueuedSendOutcome::Unavailable);
                }
                let raw = text.trim();
                plain_suffix(raw)?;
                let staged = if record.composer_pipeline == Some(true) {
                    self.run_outbound_stages(tab_id, raw.to_string())
                } else {
                    raw.to_string()
                };
                let sanitized = codeterm_pty::pty::ansi::sanitize_agent_prompt_input(&staged);
                let previous = record.edit.as_ref();
                let view = QueuedSendEdit {
                    key: QueuedSendKey {
                        version: key
                            .version
                            .checked_add(1)
                            .ok_or("Queue edit version exhausted")?,
                        ..key
                    },
                    appended: append_suffix(
                        previous.map_or("", |edit| edit.view.appended.as_str()),
                        raw,
                    )?,
                };
                let prepared = append_suffix(
                    previous.map_or("", |edit| edit.prepared.as_str()),
                    &sanitized,
                )?;
                let base = store.prepared(&record)?;
                let transport = format!("{base}\n\n{sanitized}");
                if parse_envelope_spans(&transport).iter().any(|part| {
                    part.span.end > base.len()
                        && !matches!(part.segment, MessageSegment::Text { .. })
                }) {
                    return Err(
                        "Queued additions cannot alter attachments or message markup.".into(),
                    );
                }
                let provider_id = &record.authority.provider_id;
                let paste_limit = self
                    .agent_registry
                    .with_provider(provider_id, |provider| provider.paste_limit_bytes())
                    .flatten();
                if paste_limit.is_some_and(|limit| transport.len() > limit) {
                    return Err("The addition exceeds this agent's paste limit.".into());
                }
                let follow_up = self.user_send_follow_up(provider_id, &transport);
                if !store.append_queued(
                    &record,
                    UserSendEdit {
                        view: view.clone(),
                        prepared,
                    },
                    follow_up,
                )? {
                    return Ok(QueuedSendOutcome::Unavailable);
                }
                self.wake_parked_user_send(tab_id);
                Ok(QueuedSendOutcome::Appended { edit: view })
            }
        }
    }
}

#[cfg(test)]
mod tests;
