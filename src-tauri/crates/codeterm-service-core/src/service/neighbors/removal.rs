use super::storage::Metadata;
use crate::service::ServiceContext;
use codeterm_neighbors::{wire::hex_id, Error, Result};
use serde_json::json;

struct StoppedTargets(Vec<tokio::task::JoinHandle<()>>);

impl Drop for StoppedTargets {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

impl ServiceContext {
    pub(super) async fn end_missing_neighbor_friendships(
        &self,
        metadata: &Metadata,
        list: &serde_json::Value,
        generation: u64,
    ) -> Result<()> {
        self.refresh_active_neighbor_history(metadata, list, generation)
            .await?;
        let friends = list["friends"]
            .as_array()
            .filter(|friends| friends.len() <= 200)
            .ok_or(Error::InvalidEnvelope)?;
        let settings = metadata.settings().await?;
        let candidates: std::collections::BTreeSet<_> = settings
            .bindings
            .into_keys()
            .chain(self.neighbor_listener_friendships())
            .collect();
        for friendship in candidates {
            if !friends
                .iter()
                .any(|friend| friend["friendship_id"] == friendship)
            {
                self.end_neighbor_friendship(metadata, &friendship, generation)
                    .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn end_neighbor_friendship(
        &self,
        metadata: &Metadata,
        friendship: &str,
        generation: u64,
    ) -> Result<()> {
        self.require_neighbor_history_generation(generation)?;
        let _mutation = self
            .begin_neighbor_mutation(friendship)
            .await
            .map_err(|_| Error::Gateway)?;
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        let targets = StoppedTargets(self.stop_neighbor_friendship_listeners(friendship));
        self.end_neighbor_history(metadata, friendship, generation)
            .await?;
        let mut settings = metadata.settings().await?;
        settings.paused.remove(friendship);
        self.neighbor_cell().pauses.forget(friendship);
        if let Some(tab) = settings.bindings.get(friendship).cloned() {
            self.retire_neighbor_children(&tab)
                .await
                .map_err(|_| Error::Gateway)?;
            self.require_neighbor_history_generation(generation)?;
            let tab_exists = self.mux.read().get_tab(&tab).is_some();
            if tab_exists {
                self.close_tab_async(&tab)
                    .await
                    .map_err(|_| Error::Gateway)?;
            }
            self.require_neighbor_history_generation(generation)?;
            settings.bindings.remove(friendship);
            settings.initial_contexts.remove(friendship);
            self.ui.emit(
                "neighbor-agent-changed",
                json!({"friendship_id":friendship,"gateway_tab":null}),
            );
        }
        metadata.save_settings(settings).await?;
        self.ui.emit(
            "neighbor-runtime-status",
            json!({"friendship_id":friendship,"status":"stopped","reason":"friendship_ended"}),
        );
        // Defer abort until cleanup completes: a list refresh may run inside this target.
        drop(targets);
        Ok(())
    }
}
