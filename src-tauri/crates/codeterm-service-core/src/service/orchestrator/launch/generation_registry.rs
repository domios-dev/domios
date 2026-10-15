use super::{BootStage, LaunchGeneration, ProviderAck, ProviderSessionBinding, RegisteredLaunch};

#[derive(Default)]
pub(crate) struct LaunchRegistry {
    next_generation: u64,
    tabs: std::collections::HashMap<String, RegisteredLaunch>,
}
impl LaunchRegistry {
    pub(crate) fn begin(&mut self, tab_id: &str, pty_id: String) -> LaunchGeneration {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let launch = LaunchGeneration {
            tab_id: tab_id.into(),
            pty_id,
            generation: self.next_generation,
            stage: BootStage::PtySpawned,
            reused: false,
            provider_ack: std::sync::Arc::new(ProviderAck::default()),
            binding_nonce: None,
        };
        self.tabs.insert(
            launch.tab_id.clone(),
            RegisteredLaunch {
                pty_id: launch.pty_id.clone(),
                generation: launch.generation,
                provider_ack: launch.provider_ack.clone(),
                binding_nonce: None,
            },
        );
        launch
    }

    pub(super) fn replace_pty(&mut self, launch: &mut LaunchGeneration, pty_id: String) -> bool {
        if !self.owns(launch) {
            return false;
        }
        let registered = self.tabs.get_mut(&launch.tab_id).expect("owned launch");
        registered.pty_id = pty_id.clone();
        registered.binding_nonce = None;
        registered.provider_ack = std::sync::Arc::new(ProviderAck::default());
        launch.provider_ack = registered.provider_ack.clone();
        launch.stage = BootStage::PtySpawned;
        launch.reused = false;
        launch.pty_id = pty_id;
        launch.binding_nonce = None;
        true
    }

    pub(super) fn supersede(&mut self, tab_id: &str) {
        self.tabs.remove(tab_id);
    }

    pub(super) fn owns(&self, launch: &LaunchGeneration) -> bool {
        self.tabs.get(&launch.tab_id).is_some_and(|registered| {
            registered.pty_id == launch.pty_id && registered.generation == launch.generation
        })
    }

    pub(super) fn finish(&mut self, launch: &LaunchGeneration) -> bool {
        if !self.owns(launch) {
            return false;
        }
        self.tabs.remove(&launch.tab_id);
        true
    }

    pub(crate) fn bind_provider_session(
        &mut self,
        launch: &LaunchGeneration,
        binding_nonce: String,
    ) -> bool {
        let Some(registered) = self.tabs.get_mut(&launch.tab_id) else {
            return false;
        };
        if registered.pty_id != launch.pty_id || registered.generation != launch.generation {
            return false;
        }
        registered.binding_nonce = Some(binding_nonce);
        true
    }

    pub fn record_provider_session(
        &self,
        tab_id: &str,
        binding: &ProviderSessionBinding,
    ) -> bool {
        let Some(ack) = self
            .tabs
            .get(tab_id)
            .filter(|launch| {
                launch.pty_id == binding.pty_id
                    && launch.binding_nonce.as_deref() == Some(&binding.binding_nonce)
            })
            .map(|launch| launch.provider_ack.clone())
        else {
            return false;
        };
        *ack.ready.lock() = true;
        ack.changed.notify_all();
        true
    }

    pub(super) fn take_for_pty(&mut self, pty_id: &str) -> Option<LaunchGeneration> {
        let tab_id = self
            .tabs
            .iter()
            .find(|(_, registered)| registered.pty_id == pty_id)
            .map(|(tab_id, _)| tab_id.clone())?;
        let registered = self.tabs.remove(&tab_id)?;
        Some(LaunchGeneration {
            tab_id,
            pty_id: registered.pty_id,
            generation: registered.generation,
            stage: BootStage::PtySpawned,
            reused: false,
            provider_ack: registered.provider_ack,
            binding_nonce: registered.binding_nonce,
        })
    }
}

#[cfg(test)]
mod tests;
