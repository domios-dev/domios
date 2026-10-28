use crate::service::ServiceContext;
use codeterm_neighbors::{Error, Result};
use serde_json::json;

impl ServiceContext {
    pub(super) fn require_neighbors_enabled(&self) -> Result<()> {
        if self
            .ext::<crate::config::ConfigState>()
            .lock()
            .neighbors
            .enabled
        {
            Ok(())
        } else {
            Err(Error::Disabled)
        }
    }

    pub async fn set_neighbors_enabled(&self, enabled: bool) -> std::result::Result<bool, String> {
        let mut snapshot = self.configuration();
        let changed = snapshot.neighbors.enabled != enabled;
        if changed {
            snapshot.neighbors.enabled = enabled;
            let service = self.clone();
            tokio::task::spawn_blocking(move || {
                crate::config::save_config_and_apply(
                    service.ext::<crate::config::ConfigState>(),
                    snapshot,
                )
            })
            .await
            .map_err(|_| "Neighbors setting could not be saved".to_owned())??;
            self.ui.emit(
                "config-changed",
                serde_json::to_value(self.configuration()).unwrap_or_default(),
            );
        }
        self.neighbors_config_changed();
        if !enabled {
            self.wait_neighbor_stop_local(self.neighbor_history_generation()).await?;
        }
        Ok(enabled)
    }

    pub fn neighbors_config_changed(&self) {
        let enabled = self
            .ext::<crate::config::ConfigState>()
            .lock()
            .neighbors
            .enabled;
        if enabled {
            self.restore_neighbor_updates_once();
            #[cfg(not(any(test, feature = "test-support")))]
            self.restore_neighbors_once();
        } else {
            self.pause_neighbor_reception();
            self.stop_neighbor_updates();
            self.queue_neighbor_stop_local();
        }
        self.ui
            .emit("neighbors-state-changed", json!({"enabled":enabled}));
    }
}

#[cfg(test)]
mod tests;
