use super::storage::Metadata;
use crate::service::ServiceContext;
use codeterm_neighbors::{
    wire::{hex_id, DeliveryResult, DeliveryStatus, Kind},
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
    pub retry_after_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_status: Option<DeliveryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission: Option<Submission>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Submission {
    Accepted,
    Unconfirmed,
    Refused,
}

pub(super) struct HistoryReceipt {
    pub delivery: DeliveryResult,
    pub rejection: Option<super::delivery::Rejection>,
    pub submission: Option<Submission>,
}

impl From<DeliveryResult> for HistoryReceipt {
    fn from(delivery: DeliveryResult) -> Self {
        Self {
            delivery,
            rejection: None,
            submission: None,
        }
    }
}
impl From<DeliveryStatus> for HistoryReceipt {
    fn from(status: DeliveryStatus) -> Self {
        DeliveryResult::from(status).into()
    }
}

#[derive(Default, Deserialize, Serialize)]
pub(super) struct History {
    pub ended: bool,
    #[serde(default)]
    pub active_since: Option<u64>,
    pub entries: Vec<Entry>,
}

impl History {
    fn append(&mut self, entry: Entry) -> Result<bool> {
        if self.ended {
            return Err(Error::FriendshipEnded);
        }
        if self
            .entries
            .iter()
            .any(|old| old.message_id == entry.message_id && old.direction == entry.direction)
        {
            return Ok(false);
        }
        self.entries.push(entry);
        self.reconcile_outcomes();
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

    fn reconcile_outcomes(&mut self) {
        let mut outcomes = BTreeMap::new();
        for reply in self.entries.iter().filter(|entry| {
            entry.direction == "out"
                && entry.delivery_status != Some(DeliveryStatus::NotDelivered)
                && (entry.submission == Some(Submission::Accepted)
                    || entry.delivery_status == Some(DeliveryStatus::Delivered))
        }) {
            if let Some(incoming) = reply.in_reply_to.as_ref() {
                let outcome: (String, Option<String>) = match reply.kind {
                    Kind::Answer => ("answered".into(), None),
                    Kind::Refusal => (
                        "refused".into(),
                        Some(reply.body.chars().take(256).collect::<String>()),
                    ),
                    _ => continue,
                };
                outcomes.insert(incoming.clone(), outcome);
            }
        }
        for incoming in self
            .entries
            .iter_mut()
            .filter(|entry| entry.direction == "in")
        {
            let outcome = outcomes.get(&incoming.message_id);
            incoming.outcome = outcome.map(|(outcome, _)| outcome.clone());
            incoming.reason = outcome.and_then(|(_, reason)| reason.clone());
        }
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
    pub(super) async fn outgoing_friendship(&self, message_id: &str) -> Result<Option<String>> {
        if !hex_id(message_id, 32) {
            return Err(Error::InvalidEnvelope);
        }
        let index: BTreeMap<String, String> = self
            .read(format!("history-index-{}.json", &message_id[..2]))
            .await?
            .unwrap_or_default();
        let Some(friendship) = index.get(message_id) else {
            return Ok(None);
        };
        Ok(self
            .history(friendship)
            .await?
            .entries
            .iter()
            .any(|entry| entry.direction == "out" && entry.message_id == message_id)
            .then(|| friendship.clone()))
    }

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
pub(crate) struct HistoryCell(tokio::sync::Mutex<()>);

impl ServiceContext {
    pub(super) async fn refresh_active_neighbor_history(
        &self,
        metadata: &Metadata,
        list: &Value,
        generation: u64,
    ) -> Result<()> {
        let friends = list["friends"]
            .as_array()
            .filter(|friends| friends.len() <= 200)
            .ok_or(Error::InvalidEnvelope)?;
        for friend in friends {
            let (Some(friendship), Some(since)) =
                (friend["friendship_id"].as_str(), friend["since"].as_u64())
            else {
                continue;
            };
            let _guard = self
                .extensions
                .get_or_init(HistoryCell::default)
                .0
                .lock()
                .await;
            self.require_neighbor_history_generation(generation)?;
            let mut history = metadata.history(friendship).await?;
            if history
                .active_since
                .is_some_and(|previous| since <= previous)
            {
                continue;
            }
            history.active_since = Some(since);
            history.ended = false;
            self.require_neighbor_history_generation(generation)?;
            metadata
                .write(format!("history-{friendship}.json"), history)
                .await?;
            self.ui.emit(
                "neighbor-history-changed",
                json!({"friendship_id":friendship}),
            );
        }
        Ok(())
    }

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
        let arrival = super::notifications::Arrival::from_entry(&entry);
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
        if let Some(arrival) = arrival {
            if let Err(error) = self
                .notify_neighbor_arrival(metadata, friendship, arrival, generation)
                .await
            {
                log::debug!("[neighbors] arrival notification unavailable: {error}");
            }
        }
        Ok(())
    }

    pub(super) async fn update_neighbor_history_status(
        &self,
        metadata: &Metadata,
        message_id: &str,
        receipt: HistoryReceipt,
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
        let status = match (entry.delivery_status, receipt.delivery.status) {
            (Some(DeliveryStatus::Delivered), _) => DeliveryStatus::Delivered,
            (Some(DeliveryStatus::NotDelivered), DeliveryStatus::Pending) => {
                DeliveryStatus::NotDelivered
            }
            (_, status) => status,
        };
        if entry.delivery_status == Some(status)
            && receipt.rejection.is_none()
            && receipt
                .submission
                .is_none_or(|submission| entry.submission == Some(submission))
            && receipt.delivery.reason.is_none()
        {
            return Ok(());
        }
        entry.delivery_status = Some(status);
        if status == DeliveryStatus::Delivered {
            entry.submission = Some(Submission::Accepted);
        }
        if let Some(submission) = receipt.submission {
            if entry.submission != Some(Submission::Accepted) || submission == Submission::Accepted
            {
                entry.submission = Some(submission);
            }
        }
        if let Some(reason) = receipt.delivery.reason {
            entry.reason = Some(
                match reason {
                    codeterm_neighbors::wire::DeliveryReason::KeyRevoked => "key_revoked",
                }
                .into(),
            );
        }
        if let Some(refusal) = receipt.rejection {
            entry.reason = Some(refusal.reason.into());
            entry.limit = refusal.limit;
            entry.resets_at = refusal.resets_at;
            entry.retry_after_seconds = refusal.retry_after_seconds;
        }
        history.reconcile_outcomes();
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
