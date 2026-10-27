use super::*;
use crate::service::neighbors::history::{HistoryReceipt, Submission};
use codeterm_neighbors::{
    receive::NeighborTransport, registry::DeliverySignal, wire::DeliveryStatus,
};

const MAX_PENDING: usize = 256;
const MAX_FRIENDSHIPS: usize = 200;

#[derive(Default)]
pub(super) struct Revisions {
    connected: bool,
    previous: Option<u64>,
}

impl Revisions {
    pub(super) fn observe(&mut self, revision: Option<u64>) -> Option<bool> {
        if self.connected && revision.is_some() && revision <= self.previous {
            return None;
        }
        let catch_up = !self.connected
            || self.previous.is_none() && revision.is_some()
            || self.previous.is_some() && revision.is_none()
            || matches!((self.previous, revision), (Some(previous), Some(next)) if next > previous.saturating_add(1));
        self.connected = true;
        self.previous = revision;
        Some(catch_up)
    }
}

impl Metadata {
    async fn pending_outgoing(&self, uid: &str) -> Result<Vec<String>> {
        let settings = self.settings().await?;
        let friendships: std::collections::BTreeSet<_> = settings
            .permissions
            .into_keys()
            .chain(settings.bindings.into_keys())
            .collect();
        let mut pending = Vec::new();
        for friendship in friendships.into_iter().take(MAX_FRIENDSHIPS) {
            for entry in self
                .history(&friendship)
                .await?
                .entries
                .into_iter()
                .rev()
                .take(256)
            {
                if entry.direction == "out"
                    && entry.sender_uid == uid
                    && entry.delivery_status == Some(DeliveryStatus::Pending)
                    && codeterm_neighbors::wire::hex_id(&entry.message_id, 32)
                {
                    pending.push(entry.message_id);
                    if pending.len() == MAX_PENDING {
                        return Ok(pending);
                    }
                }
            }
        }
        Ok(pending)
    }
}

impl ServiceContext {
    pub(super) async fn apply_neighbor_delivery_signal(
        &self,
        metadata: &Metadata,
        uid: &str,
        signal: &DeliverySignal,
        generation: u64,
    ) -> Result<()> {
        self.require_neighbor_history_generation(generation)?;
        if !metadata
            .history(&signal.friendship_id)
            .await?
            .entries
            .iter()
            .any(|entry| {
                entry.direction == "out"
                    && entry.sender_uid == uid
                    && entry.message_id == signal.message_id
            })
        {
            return Ok(());
        }
        self.update_neighbor_history_status(
            metadata,
            &signal.message_id,
            HistoryReceipt {
                delivery: signal.status.into(),
                rejection: None,
                submission: Some(Submission::Accepted),
            },
            generation,
        )
        .await
    }
}

impl Session {
    pub(super) async fn delivery(&self, signal: &RegistrySignal, catch_up: bool) -> Result<()> {
        self.current()?;
        if !same_session(self.account.session().await.as_ref(), &self.expected) {
            return Err(Error::Revoked);
        }
        let generation = self.service.neighbor_history_generation();
        if let Some(delivery) = &signal.delivery {
            self.service
                .apply_neighbor_delivery_signal(
                    &self.metadata,
                    &self.transport.uid,
                    delivery,
                    generation,
                )
                .await?;
        }
        if catch_up {
            for message_id in self.metadata.pending_outgoing(&self.transport.uid).await? {
                self.current()?;
                let result = self.transport.delivery_status(&message_id).await?;
                self.current()?;
                self.service
                    .update_neighbor_history_status(
                        &self.metadata,
                        &message_id,
                        result.into(),
                        generation,
                    )
                    .await?;
            }
        }
        self.current()
    }
}

#[cfg(test)]
mod tests;
