use super::*;

impl LaunchGeneration {
    pub(super) fn begin(tab_id: &str, pty_id: String) -> Self {
        LAUNCH_REGISTRY.lock().begin(tab_id, pty_id)
    }

    pub(super) fn capture_binding_nonce(
        &mut self,
        svc: &ServiceContext,
        task_identity: Option<&crate::service::startup_lease::StartupTaskIdentity>,
    ) -> Result<(), String> {
        let nonce = svc.rotate_session_binding_nonce(&self.tab_id);
        self.binding_nonce = Some(nonce.clone());
        LAUNCH_REGISTRY.lock().bind_provider_session(self, nonce);
        svc.activate_agent_startup_supervision(
            &self.tab_id,
            &self.pty_id,
            self.binding_nonce.as_deref().expect("launch binding nonce"),
            task_identity,
        )
    }

    pub(super) fn adopt_authenticated_binding(&mut self, binding_nonce: String) -> bool {
        self.binding_nonce = Some(binding_nonce.clone());
        let binding = ProviderSessionBinding {
            pty_id: self.pty_id.clone(),
            binding_nonce: binding_nonce.clone(),
        };
        let mut registry = LAUNCH_REGISTRY.lock();
        registry.bind_provider_session(self, binding_nonce)
            && registry.record_provider_session(&self.tab_id, &binding)
    }

    pub(super) fn transition(&mut self, stage: BootStage) {
        self.stage = stage;
    }

    pub(super) fn reuse(&mut self) {
        self.reused = true;
        self.transition(BootStage::AgentReady);
    }

    pub(super) fn replace(&mut self, pty_id: String) {
        let tab_id = self.tab_id.clone();
        *self = Self::begin(&tab_id, pty_id);
    }

    pub(super) fn emit(&mut self, svc: &ServiceContext, stage: BootStage) -> bool {
        if stage == BootStage::AgentReady && !self.await_provider_ack(std::time::Duration::ZERO) {
            return false;
        }
        self.transition(stage);
        self.commit_if_current(|| {
            svc.emit_boot_stage(&self.tab_id, stage, None);
        })
    }

    pub(super) fn fail(&self, svc: &ServiceContext, failure: LaunchFailure) -> bool {
        // A fault proving process death voids its binding, so it cannot outrank the failure.
        if failure.proves_process_gone() {
            svc.void_launch_session_proof(&self.tab_id);
        }
        let reason = failure.reason();
        self.finish_if_current(|| {
            svc.emit_boot_stage(&self.tab_id, BootStage::SpawnFailed, Some(&reason));
        })
    }

    pub(super) fn write_if_current<T>(&self, write: impl FnOnce() -> T) -> Option<T> {
        let registry = LAUNCH_REGISTRY.lock();
        registry.owns(self).then(write)
    }

    pub(crate) fn await_provider_ack(&self, timeout: std::time::Duration) -> bool {
        let mut ready = self.provider_ack.ready.lock();
        if *ready {
            return true;
        }
        self.provider_ack.changed.wait_for(&mut ready, timeout);
        *ready
    }

    fn commit_if_current(&self, commit: impl FnOnce()) -> bool {
        let registry = LAUNCH_REGISTRY.lock();
        if !registry.owns(self) {
            return false;
        }
        commit();
        true
    }

    fn finish_if_current(&self, commit: impl FnOnce()) -> bool {
        let mut registry = LAUNCH_REGISTRY.lock();
        if !registry.finish(self) {
            return false;
        }
        commit();
        true
    }
}
