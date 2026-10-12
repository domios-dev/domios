use super::*;

pub(super) fn reserve_binding_nonce(
    svc: &ServiceContext,
    tab_id: &str,
    resume_session_id: Option<&str>,
) -> String {
    let fork = svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
        tab.fork_source_session_id
            .as_deref()
            .is_some_and(|source| Some(source) == resume_session_id)
    });
    if crate::service::codex_rpc::lifecycle::for_tab(tab_id).is_some() {
        return svc.ensure_session_binding_nonce(tab_id);
    }
    match resume_session_id.filter(|_| !fork) {
        Some(session_id) => {
            crate::service::launch_session_detection::register_resume_launch_marker(
                tab_id, session_id,
            );
            svc.reserve_agent_launch_binding_nonce(tab_id, Some(session_id))
        }
        None => svc.reserve_agent_launch_binding_nonce(tab_id, None),
    }
}

impl LaunchGeneration {
    pub(super) fn begin(tab_id: &str, pty_id: String) -> Self {
        LAUNCH_REGISTRY.lock().begin(tab_id, pty_id)
    }

    pub(super) fn capture_binding_nonce(
        &mut self,
        svc: &ServiceContext,
        task_identity: Option<&crate::service::startup_lease::StartupTaskIdentity>,
        public_send_key: Option<crate::service::public_send_readiness::PublicSendGateKey>,
        origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
        resume_session_id: Option<&str>,
        reserved_nonce: Option<String>,
    ) -> Result<(), String> {
        let sidecar = crate::service::codex_rpc::lifecycle::for_tab(&self.tab_id);
        let current = svc
            .mux
            .read()
            .get_tab(&self.tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone());
        // A launch path may re-mint the binding after the early reservation (Codex RPC does);
        // then this launch takes a fresh nonce and sends pinned to the old one expire honestly.
        let nonce = match reserved_nonce {
            Some(reserved) if current.as_deref() == Some(reserved.as_str()) => reserved,
            _ => reserve_binding_nonce(svc, &self.tab_id, resume_session_id),
        };
        if !svc
            .mux
            .read()
            .get_tab(&self.tab_id)
            .is_some_and(|tab| tab.session_binding_nonce.as_deref() == Some(nonce.as_str()))
        {
            return Err("launch binding changed before registration".into());
        }
        if sidecar.is_none() {
            crate::service::codex_rpc::lifecycle::plain_launch(svc, &self.tab_id, &nonce);
        }
        crate::service::structured_prompt::bind_launch(&self.tab_id, &nonce);
        svc.arm_spawn_reply_relay(&self.tab_id, &nonce);
        self.binding_nonce = Some(nonce.clone());
        LAUNCH_REGISTRY
            .lock()
            .bind_provider_session(self, nonce.clone());
        if let Some(sidecar) = sidecar {
            svc.set_agent_session_id(&self.tab_id, Some(sidecar.thread.lock().clone()));
            if let Some(tab) = svc.mux.write().get_tab_mut(&self.tab_id) {
                tab.reported_session_binding_nonce = Some(nonce.clone());
            }
            record_provider_session(
                &self.tab_id,
                &ProviderSessionBinding {
                    pty_id: self.pty_id.clone(),
                    binding_nonce: nonce,
                },
            );
        }
        // A relaunched Worker keeps its persisted task binding, but its dispatch
        // handshake was settled by the launch that first started it.
        let startup_owed = !origin.relaunches_existing_agent()
            || task_identity.is_some()
            || svc.worker_ack.startup_handshake_owed(&self.tab_id);
        if startup_owed {
            svc.activate_agent_startup_supervision(
                &self.tab_id,
                &self.pty_id,
                self.binding_nonce.as_deref().expect("launch binding nonce"),
                task_identity,
            )?;
        }
        svc.bind_public_send_launch_attempt(
            crate::service::launch_readiness::LaunchAttempt {
                tab_id: self.tab_id.clone(),
                pty_id: self.pty_id.clone(),
                binding_nonce: self.binding_nonce.clone().expect("launch binding nonce"),
            },
            public_send_key,
        );
        Ok(())
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

    pub(super) fn replace(&mut self, pty_id: String) -> bool {
        LAUNCH_REGISTRY.lock().replace_pty(self, pty_id)
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

    pub(super) fn emit_first_turn_input_ready(&mut self, svc: &ServiceContext) -> bool {
        self.transition(BootStage::AgentReady);
        self.commit_if_current(|| {
            svc.emit_boot_stage(&self.tab_id, BootStage::AgentReady, None);
        })
    }

    pub(super) fn fail(&self, svc: &ServiceContext, failure: LaunchFailure) -> bool {
        if !LAUNCH_REGISTRY.lock().owns(self) {
            return false;
        }
        if matches!(failure, LaunchFailure::SessionBindingNotObserved(_))
            && self.await_provider_ack(std::time::Duration::ZERO)
            && svc
                .observe_provider_process_tree(&self.tab_id, &self.pty_id)
                .is_some_and(|tree| {
                    tree.state() == crate::service::process_tree::ProviderTreeState::Present
                })
        {
            return self.commit_if_current(|| {
                svc.emit_boot_stage(&self.tab_id, BootStage::AgentReady, None)
            });
        }
        if matches!(failure, LaunchFailure::SessionBindingNotObserved(_))
            && svc.resumed_provider_is_live(&self.tab_id, &self.pty_id)
        {
            if let Some(attempt) = self.attempt() {
                svc.record_provisional_launch(
                    &attempt,
                    BootStage::LaunchBindingUnconfirmed,
                    &failure.reason(),
                );
                return true;
            }
        }
        if let Some(nonce) = self.binding_nonce.as_deref() {
            if crate::service::codex_rpc::recovery::recovering(&self.tab_id, nonce) {
                return true;
            }
            if crate::service::codex_rpc::recovery::try_start(
                svc,
                &self.tab_id,
                nonce,
                &failure.reason(),
            ) {
                return true;
            }
        }
        let reason = failure.reason();
        crate::service::launch_diagnostics::log_failure_screen(
            svc,
            &self.tab_id,
            &self.pty_id,
            &reason,
        );
        log::warn!(
            "[launch] Tab {} launch failed on PTY {} at {:?}: {reason}",
            self.tab_id,
            self.pty_id,
            self.stage
        );
        if let Some(attempt) = self.attempt() {
            svc.retire_public_send_launch_attempt(&attempt);
        }
        self.finish_if_current(|| {
            if failure.proves_process_gone() {
                svc.void_launch_session_proof(&self.tab_id);
            }
            if let Some(tab) = svc.mux.write().get_tab_mut(&self.tab_id) {
                tab.agent_booting = false;
            }
            svc.emit_boot_stage(&self.tab_id, BootStage::SpawnFailed, Some(&reason));
            crate::service::task_files::release_tab(svc, &self.tab_id);
        })
    }

    pub(super) fn await_remote_attachment(&self, svc: &ServiceContext) -> bool {
        if crate::service::codex_rpc::lifecycle::for_tab(&self.tab_id).is_none() {
            return true;
        }
        let Some(nonce) = self.binding_nonce.as_deref() else {
            return false;
        };
        if crate::service::codex_rpc::lifecycle::wait_attached(
            &self.tab_id,
            nonce,
            std::time::Duration::from_secs(30),
        ) {
            return true;
        }
        crate::service::codex_rpc::recovery::try_start(
            svc,
            &self.tab_id,
            nonce,
            "remote TUI attachment failed",
        );
        if crate::service::codex_rpc::recovery::wait_recovery(&self.tab_id, nonce) {
            return true;
        }
        self.fail(
            svc,
            LaunchFailure::ProviderInputNotReady("remote TUI attachment failed".into()),
        );
        false
    }

    pub(super) fn complete_structured_startup(&self, svc: &ServiceContext, consumed: bool) {
        if let Some(nonce) = self.binding_nonce.as_deref().filter(|_| consumed) {
            crate::service::codex_rpc::lifecycle::startup_complete(svc, &self.tab_id, nonce);
        }
    }

    pub(super) fn attempt(&self) -> Option<crate::service::launch_readiness::LaunchAttempt> {
        Some(crate::service::launch_readiness::LaunchAttempt {
            tab_id: self.tab_id.clone(),
            pty_id: self.pty_id.clone(),
            binding_nonce: self.binding_nonce.clone()?,
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

    pub(super) fn commit_if_current(&self, commit: impl FnOnce()) -> bool {
        let registry = LAUNCH_REGISTRY.lock();
        if !registry.owns(self) {
            return false;
        }
        commit();
        true
    }

    pub(super) fn finish_if_current(&self, commit: impl FnOnce()) -> bool {
        let mut registry = LAUNCH_REGISTRY.lock();
        if !registry.finish(self) {
            return false;
        }
        commit();
        true
    }
}
