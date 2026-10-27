use super::storage::Metadata;
use crate::service::ServiceContext;
use codeterm_neighbors::{
    wire::{hex_id, DeliveryStatus, Kind},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 768 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub(super) enum Author {
    Person(String),
    Agent { agent: String, name: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Entry {
    pub direction: String,
    pub author: Author,
    pub sender_uid: String,
    pub body: String,
    pub at: u64,
    pub message_id: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_status: Option<DeliveryStatus>,
}

#[derive(Default, Deserialize, Serialize)]
pub(super) struct History {
    pub ended: bool,
    pub entries: Vec<Entry>,
}

impl History {
    fn append(&mut self, entry: Entry) -> Result<bool> {
        if self.ended {
            return Err(Error::Revoked);
        }
        if self
            .entries
            .iter()
            .any(|old| old.message_id == entry.message_id && old.direction == entry.direction)
        {
            return Ok(false);
        }
        if entry.direction == "out" {
            if let Some(incoming) = self.entries.iter_mut().find(|old| {
                old.direction == "in" && Some(&old.message_id) == entry.in_reply_to.as_ref()
            }) {
                match entry.kind {
                    Kind::Refusal => {
                        incoming.outcome = Some("refused".into());
                        incoming.reason = Some(entry.body.chars().take(256).collect());
                    }
                    Kind::Answer => {
                        incoming.outcome = Some("answered".into());
                        incoming.reason = None;
                    }
                    _ => {}
                }
            }
        }
        self.entries.push(entry);
        while self.entries.len() > MAX_ENTRIES
            || serde_json::to_vec(self)
                .map_err(|_| Error::CursorStore)?
                .len()
                > MAX_BYTES
        {
            self.entries.remove(0);
        }
        Ok(true)
    }

    fn page(&self, before: Option<&str>, limit: usize) -> Value {
        let end = before.map_or(self.entries.len(), |id| {
            self.entries
                .iter()
                .position(|entry| entry.message_id == id)
                .unwrap_or(0)
        });
        let start = end.saturating_sub(limit.clamp(1, 100));
        let entries: Vec<_> = self.entries[start..end].iter().rev().collect();
        let next = (start > 0).then(|| self.entries[start].message_id.clone());
        json!({"entries":entries,"next_before":next,"ended":self.ended})
    }
}

impl Metadata {
    async fn history(&self, friendship: &str) -> Result<History> {
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        Ok(self
            .read(format!("history-{friendship}.json"))
            .await?
            .unwrap_or_default())
    }

    pub(super) async fn history_page(
        &self,
        friendship: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        if before.is_some_and(|id| !hex_id(id, 32)) {
            return Err(Error::InvalidEnvelope);
        }
        let history = self.history(friendship).await?;
        let before = before.map(str::to_owned);
        tokio::task::spawn_blocking(move || history.page(before.as_deref(), limit))
            .await
            .map_err(|_| Error::CursorStore)
    }

    pub(super) async fn has_incoming(&self, friendship: &str, message_id: &str) -> Result<bool> {
        Ok(self
            .history(friendship)
            .await?
            .entries
            .iter()
            .any(|entry| entry.direction == "in" && entry.message_id == message_id))
    }
}

#[derive(Default)]
struct HistoryCell(tokio::sync::Mutex<()>);

impl ServiceContext {
    pub(super) fn neighbor_author(&self, caller: Option<&str>) -> Result<Author> {
        match caller {
            None => Ok(Author::Person("me".into())),
            Some(id) => {
                let mux = self.mux.read();
                let tab = mux.get_tab(id).ok_or(Error::Gateway)?;
                Ok(Author::Agent {
                    agent: id.into(),
                    name: tab.title.clone(),
                })
            }
        }
    }

    pub(super) async fn append_neighbor_history(
        &self,
        metadata: &Metadata,
        friendship: &str,
        entry: Entry,
        generation: u64,
    ) -> Result<()> {
        let _guard = self
            .extensions
            .get_or_init(HistoryCell::default)
            .0
            .lock()
            .await;
        self.require_neighbor_history_generation(generation)?;
        let mut history = metadata.history(friendship).await?;
        let outgoing = entry.direction == "out";
        let message_id = entry.message_id.clone();
        let (history, changed) = tokio::task::spawn_blocking(move || {
            let changed = history.append(entry)?;
            Ok::<_, Error>((history, changed))
        })
        .await
        .map_err(|_| Error::CursorStore)??;
        if !changed {
            return Ok(());
        }
        self.require_neighbor_history_generation(generation)?;
        metadata
            .write(format!("history-{friendship}.json"), history)
            .await?;
        if outgoing {
            if !hex_id(&message_id, 32) {
                return Err(Error::InvalidEnvelope);
            }
            let index_path = format!("history-index-{}.json", &message_id[..2]);
            let mut index: BTreeMap<String, String> =
                metadata.read(index_path.clone()).await?.unwrap_or_default();
            let kept: std::collections::BTreeSet<_> = metadata
                .history(friendship)
                .await?
                .entries
                .into_iter()
                .map(|entry| entry.message_id)
                .collect();
            index.retain(|id, owner| owner != friendship || kept.contains(id));
            index.insert(message_id, friendship.into());
            self.require_neighbor_history_generation(generation)?;
            metadata.write(index_path, index).await?;
        }
        self.ui.emit(
            "neighbor-history-changed",
            json!({"friendship_id":friendship}),
        );
        Ok(())
    }

    pub(super) async fn update_neighbor_history_status(
        &self,
        metadata: &Metadata,
        message_id: &str,
        status: DeliveryStatus,
        refusal: Option<(u64, u64)>,
        generation: u64,
    ) -> Result<()> {
        let _guard = self
            .extensions
            .get_or_init(HistoryCell::default)
            .0
            .lock()
            .await;
        self.require_neighbor_history_generation(generation)?;
        if !hex_id(message_id, 32) {
            return Err(Error::InvalidEnvelope);
        }
        let index: BTreeMap<String, String> = metadata
            .read(format!("history-index-{}.json", &message_id[..2]))
            .await?
            .unwrap_or_default();
        let Some(friendship) = index.get(message_id) else {
            return Ok(());
        };
        let mut history = metadata.history(friendship).await?;
        let Some(entry) = history
            .entries
            .iter_mut()
            .find(|entry| entry.direction == "out" && entry.message_id == message_id)
        else {
            return Ok(());
        };
        if entry.delivery_status == Some(status) && refusal.is_none() {
            return Ok(());
        }
        entry.delivery_status = Some(status);
        if let Some((limit, resets_at)) = refusal {
            entry.reason = Some("neighbor_daily_limit".into());
            entry.limit = Some(limit);
            entry.resets_at = Some(resets_at);
        }
        self.require_neighbor_history_generation(generation)?;
        metadata
            .write(format!("history-{friendship}.json"), history)
            .await?;
        self.ui.emit(
            "neighbor-history-changed",
            json!({"friendship_id":friendship}),
        );
        Ok(())
    }

    pub(super) async fn end_neighbor_history(
        &self,
        metadata: &Metadata,
        friendship: &str,
        generation: u64,
    ) -> Result<()> {
        let _guard = self
            .extensions
            .get_or_init(HistoryCell::default)
            .0
            .lock()
            .await;
        self.require_neighbor_history_generation(generation)?;
        let mut history = metadata.history(friendship).await?;
        if history.ended {
            return Ok(());
        }
        history.ended = true;
        self.require_neighbor_history_generation(generation)?;
        metadata
            .write(format!("history-{friendship}.json"), history)
            .await?;
        self.ui.emit(
            "neighbor-history-changed",
            json!({"friendship_id":friendship}),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
