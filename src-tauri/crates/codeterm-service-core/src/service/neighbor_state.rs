use super::{
    spawn::{SpawnTabSpec, TabCommand},
    ServiceContext,
};
pub use crate::multiplexer::types::NeighborAuthority;
use std::{collections::HashMap, sync::Arc};
#[derive(Clone, Copy, Default)]
pub struct Observation {
    pub paused: bool,
    pub version: u64,
}

#[derive(Default)]
pub struct State(pub parking_lot::Mutex<HashMap<String, Observation>>);

impl State {
    pub fn clear(&self) {
        self.0.lock().clear();
    }
    pub fn forget(&self, friendship: &str) {
        self.0.lock().remove(friendship);
    }
    pub fn snapshot(&self) -> HashMap<String, u64> {
        self.0
            .lock()
            .iter()
            .map(|(id, value)| (id.clone(), value.version))
            .collect()
    }
    pub fn version(&self, friendship: &str) -> u64 {
        self.0
            .lock()
            .get(friendship)
            .map_or(0, |value| value.version)
    }
    pub fn get(&self, friendship: &str) -> Option<bool> {
        self.0.lock().get(friendship).map(|value| value.paused)
    }
    pub fn observe(&self, friendship: &str, paused: bool, expected: Option<u64>) -> Option<bool> {
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

#[derive(Default)]
pub struct LaunchState {
    pub gate: parking_lot::Mutex<()>,
    pub mutations: parking_lot::Mutex<HashMap<String, usize>>,
}

pub struct Mutation {
    state: Arc<LaunchState>,
    friendship: String,
}

impl Drop for Mutation {
    fn drop(&mut self) {
        let mut mutations = self.state.mutations.lock();
        if let Some(count) = mutations.get_mut(&self.friendship) {
            *count -= 1;
            if *count == 0 {
                mutations.remove(&self.friendship);
            }
        }
    }
}

pub fn begin(state: Arc<LaunchState>, friendship: String) -> Mutation {
    {
        // Finish any launch before collecting descendants; new launches then see the mutation.
        let _gate = state.gate.lock();
        *state
            .mutations
            .lock()
            .entry(friendship.clone())
            .or_default() += 1;
    }
    Mutation { state, friendship }
}

impl ServiceContext {
    pub fn neighbor_pauses(&self) -> &State {
        self.ext::<State>()
    }
    pub fn neighbor_launches(&self) -> &Arc<LaunchState> {
        self.ext::<Arc<LaunchState>>()
    }
    pub fn neighbor_friend_paused(&self, friendship: &str) -> bool {
        self.neighbor_pauses().get(friendship).unwrap_or(false)
    }
    pub fn record_neighbor_pause_state(
        &self,
        uid: &str,
        friendship: &str,
        paused: bool,
        expected: Option<u64>,
    ) -> bool {
        if self.neighbor_pauses().observe(friendship, paused, expected) != Some(true) {
            return false;
        }
        {
            let mut mux = self.mux.write();
            let ids: Vec<_> = mux
                .snapshot()
                .tabs
                .iter()
                .filter(|(_, tab)| {
                    tab.neighbor_authority.as_ref().is_some_and(|authority| {
                        authority.uid == uid && authority.friendship_id == friendship
                    })
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                if let Some(tab) = mux.get_tab_mut(&id) {
                    if !paused || !tab.is_chat_backend() || tab.agent_session_id.is_some() {
                        tab.frozen = paused;
                    }
                }
            }
        }
        true
    }
    pub fn neighbor_tab_launch_guard(
        &self,
        spec: &SpawnTabSpec,
    ) -> Result<Option<parking_lot::MutexGuard<'_, ()>>, String> {
        let spawner = match &spec.command {
            TabCommand::Agent(agent) => agent.spawned_by_tab_id.as_deref(),
            _ => spec
                .chatbackend_launch
                .as_ref()
                .and_then(|launch| launch.params.spawned_by_tab_id.as_deref()),
        };
        if self
            .neighbors()
            .neighbor_launch_caller(&[
                spawner,
                spec.creator_tab_id.as_deref(),
                spec.parent_tab_id.as_deref(),
            ])?
            .is_none()
        {
            return Ok(None);
        }
        Ok(Some(self.neighbor_launches().gate.lock()))
    }
}
