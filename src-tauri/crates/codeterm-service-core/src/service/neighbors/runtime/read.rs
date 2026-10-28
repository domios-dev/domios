use super::*;
use crate::service::neighbors::history::HistoryCell;

impl ServiceContext {
    pub async fn neighbor_mark_read(
        &self,
        friendship_id: String,
        through_message_id: String,
    ) -> Result<Value, String> {
        if !hex_id(&friendship_id, 64) || !hex_id(&through_message_id, 32) {
            return Err(Error::InvalidRequest.to_string());
        }
        let expected_generation = self.neighbor_history_generation();
        match self
            .neighbor_operation(owner::Operation::Read {
                friendship_id,
                through_message_id,
                expected_generation,
            })
            .await?
        {
            owner::Outcome::Value(value) => Ok(value),
        }
    }

    pub(super) async fn neighbor_read_owned(
        &self,
        friendship: String,
        through: String,
        generation: u64,
    ) -> Result<Value, String> {
        self.require_neighbors_enabled()
            .map_err(|error| error.to_string())?;
        let (transport, _, _, metadata) = self.neighbor_ports().await?;
        let _guard = self
            .extensions
            .get_or_init(HistoryCell::default)
            .0
            .lock()
            .await;
        self.require_neighbor_generation(generation)?;
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let their_paused = self
            .neighbor_cell()
            .registry_snapshot
            .lock()
            .as_ref()
            .and_then(|list| list["friends"].as_array())
            .is_some_and(|friends| {
                friends.iter().any(|friend| {
                    friend["friendship_id"] == friendship && friend["their_paused"] == true
                })
            });
        let mut history = metadata
            .history(&friendship)
            .await
            .map_err(|error| error.to_string())?;
        let enabled = self.require_neighbors_enabled().is_ok();
        let paused = self.neighbor_effective_pause(&settings, &friendship) || their_paused;
        let requested = history.request_read_through(&through, enabled, paused);
        let through = history.pending_read_through().unwrap_or(through);
        let reserved = history.reserve_read_through(
            &through,
            enabled,
            paused,
        );
        if !reserved && !requested {
            return Ok(
                json!({"friendship_id":friendship,"through_message_id":through,"sent":false}),
            );
        }
        // Persist before the callable: an uncertain response must never replay a read receipt.
        metadata
            .write(format!("history-{friendship}.json"), history)
            .await
            .map_err(|error| error.to_string())?;
        drop(_guard);
        if !reserved {
            return Ok(json!({"friendship_id":friendship,"through_message_id":through,"sent":false}));
        }
        self.require_neighbors_enabled()
            .map_err(|error| error.to_string())?;
        self.require_neighbor_generation(generation)?;
        if self.neighbor_friend_paused(&friendship) {
            return Ok(
                json!({"friendship_id":friendship,"through_message_id":through,"sent":false}),
            );
        }
        let result = transport.calls.invoke("friendsRead", json!({"installation_id":transport.installation_id,"friendship_id":friendship,"through_message_id":through})).await.map_err(|error| error.to_string())?;
        self.require_neighbor_generation(generation)?;
        Ok(result)
    }

    pub(in crate::service::neighbors) fn queue_neighbor_read(
        &self,
        friendship: &str,
        through: String,
        generation: u64,
    ) {
        if self.require_neighbors_enabled().is_err() {
            return;
        }
        let service = self.clone();
        let friendship = friendship.to_owned();
        tokio::spawn(async move {
            if service.require_neighbor_generation(generation).is_err() {
                return;
            }
            if let Err(error) = service
                .neighbor_operation(owner::Operation::Read {
                    friendship_id: friendship,
                    through_message_id: through,
                    expected_generation: generation,
                })
                .await
            {
                log::debug!("[neighbors] read receipt unavailable: {error}");
            }
        });
    }

    pub(in crate::service::neighbors) async fn commit_neighbor_incoming_ack(&self, metadata: &Metadata, friendship: &str, message: &str, generation: u64) -> codeterm_neighbors::Result<()> {
        let guard = self.extensions.get_or_init(HistoryCell::default).0.lock().await;
        self.require_neighbor_history_generation(generation)?;
        let mut history = metadata.history(friendship).await?;
        if history.ended { return Ok(()); }
        let changed = history.acknowledge_message(message);
        let pending = history.pending_read_through();
        if changed { metadata.write(format!("history-{friendship}.json"),history).await?; }
        drop(guard);
        if let Some(through) = pending { self.queue_neighbor_read(friendship,through,generation); }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
