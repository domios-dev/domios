use super::{
    storage::{Metadata, Settings},
    ServiceContext,
};
use codeterm_neighbors::{adapter::FirebaseTransport, Error, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, Default)]
struct Observation {
    paused: bool,
    version: u64,
}

#[derive(Default)]
pub(super) struct State(parking_lot::Mutex<HashMap<String, Observation>>);

impl State {
    pub(super) fn clear(&self) {
        self.0.lock().clear();
    }
    pub(super) fn forget(&self, friendship: &str) {
        self.0.lock().remove(friendship);
    }
    pub(super) fn snapshot(&self) -> HashMap<String, u64> {
        self.0
            .lock()
            .iter()
            .map(|(id, value)| (id.clone(), value.version))
            .collect()
    }
    fn version(&self, friendship: &str) -> u64 {
        self.0
            .lock()
            .get(friendship)
            .map_or(0, |value| value.version)
    }
    fn get(&self, friendship: &str) -> Option<bool> {
        self.0.lock().get(friendship).map(|value| value.paused)
    }
    fn observe(&self, friendship: &str, paused: bool, expected: Option<u64>) -> Option<bool> {
        let mut values = self.0.lock();
        if expected.is_some_and(|expected| {
            expected != values.get(friendship).map_or(0, |value| value.version)
        }) {
            return None;
        }
        let value = values.entry(friendship.into()).or_default();
        let changed = value.paused != paused;
        value.paused = paused;
        value.version = value.version.wrapping_add(1);
        Some(changed)
    }
}

impl ServiceContext {
    pub(crate) fn neighbor_friend_paused(&self, friendship: &str) -> bool {
        self.neighbor_cell().pauses.get(friendship).unwrap_or(false)
    }

    pub(super) fn neighbor_effective_pause(&self, settings: &Settings, friendship: &str) -> bool {
        self.neighbor_cell()
            .pauses
            .get(friendship)
            .unwrap_or_else(|| settings.paused.contains(friendship))
    }

    pub(super) fn observe_neighbor_pause(&self, uid: &str, friendship: &str, paused: bool) {
        self.record_neighbor_pause(uid, friendship, paused, None);
    }

    fn record_neighbor_pause(
        &self,
        uid: &str,
        friendship: &str,
        paused: bool,
        expected: Option<u64>,
    ) {
        if self
            .neighbor_cell()
            .pauses
            .observe(friendship, paused, expected)
            != Some(true)
        {
            return;
        }
        {
            let mut mux = self.mux.write();
            let ids: Vec<_> = mux
                .snapshot()
                .tabs
                .iter()
                .filter_map(|(id, tab)| {
                    tab.neighbor_authority
                        .as_ref()
                        .is_some_and(|authority| {
                            authority.uid == uid && authority.friendship_id == friendship
                        })
                        .then(|| id.clone())
                })
                .collect();
            for id in ids {
                if let Some(tab) = mux.get_tab_mut(&id) {
                    if !paused || !tab.is_chat_backend() || tab.agent_session_id.is_some() {
                        tab.frozen = paused;
                    }
                }
            }
        }
        self.stop_neighbor_listeners();
        self.patch_neighbor_registry_pause(friendship, paused);
        self.emit_state_changed(crate::service::MuxEmitCause::TabMetadata);
        self.ui.emit(
            "neighbor-friend-paused",
            json!({"friendship_id":friendship,"paused":paused}),
        );
    }

    pub(super) async fn persist_neighbor_pauses(
        &self,
        metadata: &Metadata,
        generation: u64,
    ) -> Result<()> {
        let mut settings = metadata.settings().await?;
        let before = settings.paused.clone();
        let values = self.neighbor_cell().pauses.0.lock().clone();
        for (friendship, value) in values {
            if value.paused {
                settings.paused.insert(friendship);
            } else {
                settings.paused.remove(&friendship);
            }
        }
        self.require_neighbor_history_generation(generation)?;
        if settings.paused == before {
            Ok(())
        } else {
            metadata.save_settings(settings).await
        }
    }

    pub(super) async fn set_neighbor_paused(
        &self,
        transport: &FirebaseTransport,
        metadata: &Metadata,
        friendship: &str,
        paused: bool,
        generation: u64,
    ) -> Result<Value> {
        let version = self.neighbor_cell().pauses.version(friendship);
        transport.set_paused(friendship, paused).await?;
        self.require_neighbor_history_generation(generation)?;
        self.record_neighbor_pause(&transport.uid, friendship, paused, Some(version));
        self.persist_neighbor_pauses(metadata, generation).await?;
        self.settle_neighbor_pause(metadata, friendship).await?;
        Ok(json!({"friendship_id":friendship,"paused":self.neighbor_friend_paused(friendship)}))
    }

    pub(super) async fn settle_neighbor_pause(
        &self,
        metadata: &Metadata,
        friendship: &str,
    ) -> Result<()> {
        let settings = metadata.settings().await?;
        let Some(tab) = settings.bindings.get(friendship) else {
            return Ok(());
        };
        let paused = self.neighbor_effective_pause(&settings, friendship);
        if paused {
            self.pause_neighbor_tab(tab, friendship).await?;
        }
        Ok(())
    }

    pub(super) async fn pause_neighbor_tab(&self, tab: &str, friendship: &str) -> Result<()> {
        {
            let mut mux = self.mux.write();
            let Some(target) = mux.get_tab_mut(tab) else {
                return Ok(());
            };
            if target.is_chat_backend() && target.agent_session_id.is_none() {
                return Ok(());
            }
            target.frozen = true;
        }
        crate::service::stop_execution::stop_agent_turn(self.clone(), tab.to_owned())
            .await
            .map_err(|error| {
                log::debug!("[neighbors] pausing concierge tab={tab}: {error}");
                Error::Gateway
            })?;
        if let Some(tab) = self.mux.write().get_tab_mut(tab) {
            tab.frozen = self.neighbor_friend_paused(friendship);
        }
        Ok(())
    }

    pub(super) fn remember_neighbor_pause_settings(&self, uid: &str, settings: &Settings) {
        for friendship in &settings.paused {
            self.record_neighbor_pause(uid, friendship, true, Some(0));
        }
    }

    pub(super) fn reconcile_neighbor_pauses(
        &self,
        uid: &str,
        list: &Value,
        versions: &HashMap<String, u64>,
    ) -> Result<()> {
        let friends = list["friends"]
            .as_array()
            .filter(|friends| friends.len() <= 200)
            .ok_or(Error::InvalidEnvelope)?;
        for friend in friends {
            let friendship = friend["friendship_id"]
                .as_str()
                .filter(|id| codeterm_neighbors::wire::hex_id(id, 64))
                .ok_or(Error::InvalidEnvelope)?;
            if let Some(paused) = friend["my_paused"].as_bool() {
                self.record_neighbor_pause(
                    uid,
                    friendship,
                    paused,
                    Some(versions.get(friendship).copied().unwrap_or(0)),
                );
            }
        }
        Ok(())
    }

    pub(super) fn project_neighbor_pauses(&self, list: &mut Value, settings: &Settings) {
        if let Some(friends) = list["friends"].as_array_mut() {
            for friend in friends {
                if let Some(id) = friend["friendship_id"].as_str() {
                    let paused = self.neighbor_effective_pause(settings, id);
                    friend["my_paused"] = json!(paused);
                    friend["paused"] = json!(paused);
                    if friend["their_paused"].as_bool().is_none() {
                        friend["their_paused"] = json!(false);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
