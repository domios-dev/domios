use super::ServiceContext;
use crate::service::session_model::clear_runtime_model_evidence;

#[cfg(test)]
pub fn clear_agent_identity_for_lifecycle_exit(tab: &mut crate::multiplexer::types::Tab) -> bool {
    let prompt_identity = crate::service::prompt_resolution::clear_tab_prompt(Some(tab));
    clear_agent_identity_after_prompt_clear(tab, prompt_identity.is_some())
}

fn clear_agent_identity_after_prompt_clear(
    tab: &mut crate::multiplexer::types::Tab,
    had_prompt: bool,
) -> bool {
    if tab.boot_stage == Some(crate::service::boot_stage::BootStage::SpawnFailed) {
        if let Some(provider) = tab.agent_type.clone() {
            tab.restorable_launch.failed_provider = Some(provider);
        }
    }
    if let (Some(provider_id), Some(session_id)) =
        (tab.agent_type.as_deref(), tab.agent_session_id.as_deref())
    {
        crate::service::integration_silence::clear_hook_binding_fallback_session(
            provider_id,
            session_id,
        );
    }
    let has_identity = tab.agent_type.is_some()
        || tab.agent_type_locked
        || tab.agent_session_id.is_some()
        || tab.agent_session_name.is_some()
        || tab.session_binding_nonce.is_some()
        || tab.reported_session_binding_nonce.is_some()
        || tab.agent_state.is_some()
        || had_prompt;
    if !has_identity {
        return false;
    }
    let pending_model_switch_observation = tab
        .pending_model_switch_observation
        .as_ref()
        .filter(|pending| tab.agent_session_id.as_deref() == Some(pending.session_id.as_str()))
        .cloned();
    tab.agent_type = None;
    tab.agent_type_locked = false;
    tab.agent_set_at = None;
    tab.agent_booting = false;
    // Preserve session lineage before retiring the nonce, so a resumed session
    // can still prove it belongs to the same agent after relaunch.
    tab.remember_superseded_binding_nonce();
    tab.agent_session_id = None;
    tab.agent_session_name = None;
    tab.session_binding_nonce = None;
    tab.reported_session_binding_nonce = None;
    tab.agent_state = None;
    tab.exit_suspected_at = None;
    // Provider and model are one identity; clearing only the provider leaves a
    // stale model label attached to the tab.
    clear_runtime_model_evidence(tab);
    tab.pending_model_switch_observation = pending_model_switch_observation;
    crate::service::boot_stage::apply_boot_stage(
        tab,
        codeterm_types::pty::BootStage::ShellReady,
        None,
    );
    true
}

#[derive(Default)]
struct ClearedAgentIdentity {
    cleared: bool,
    prompt_identity: Option<codeterm_types::pty::PromptIdentity>,
    pty_id: Option<String>,
}

fn clear_agent_identity_with_prompt(
    tab: &mut crate::multiplexer::types::Tab,
) -> ClearedAgentIdentity {
    let pty_id = tab.pty_id.clone();
    let identity = crate::service::prompt_resolution::clear_tab_prompt(Some(tab));
    let cleared = clear_agent_identity_after_prompt_clear(tab, identity.is_some());
    ClearedAgentIdentity {
        cleared,
        prompt_identity: identity.filter(|_| cleared),
        pty_id: pty_id.filter(|_| cleared),
    }
}

impl ServiceContext {
    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit_with_cause(
        &self,
        tab_id: &str,
        cause: &str,
    ) -> bool {
        self.clear_tab_agent_identity_for_lifecycle_exit_during(
            tab_id,
            cause,
            crate::crash::shutdown::active_shutdown(),
        )
    }

    pub(crate) fn clear_tab_agent_identity_after_confirmed_exit(
        &self,
        tab_id: &str,
        cause: &str,
    ) -> bool {
        self.clear_tab_agent_identity_after_confirmed_exit_with_evidence(
            tab_id,
            cause,
            "confirmed_tab_pty_exit_event",
        )
    }

    fn clear_tab_agent_identity_after_confirmed_exit_with_evidence(
        &self,
        tab_id: &str,
        cause: &str,
        tree_evidence: &str,
    ) -> bool {
        if crate::crash::shutdown::active_shutdown().is_some() {
            return false;
        }
        let exit = {
            let pty_id = self
                .mux
                .read()
                .get_tab(tab_id)
                .and_then(|tab| tab.pty_id.clone());
            if self.tab_chat_backing(tab_id).is_some() {
                None
            } else {
                pty_id.and_then(|pty_id| {
                    let attempt = crate::service::launch_readiness::LaunchAttempt::current(
                        &self.mux, tab_id, &pty_id,
                    )?;
                    let proof =
                        crate::service::launch_readiness::LaunchExitProof::from_confirmed_pty_exit(
                            &attempt,
                            tree_evidence,
                        );
                    self.claim_launch_exit(tab_id, Some(&pty_id), None, &proof)
                })
            }
        };
        if !self.lifecycle_clear_allowed(
            tab_id,
            cause,
            crate::crash::shutdown::active_shutdown().as_ref(),
        ) {
            if let Some(exit) = exit {
                self.publish_launch_exit(&exit);
            }
            return false;
        }
        let cleared = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .map(clear_agent_identity_with_prompt)
                .unwrap_or_default()
        };
        crate::service::launch_readiness::owner_prompt_cleared(tab_id);
        let finished =
            self.finish_agent_identity_clear(tab_id, cause, cleared, "pty-exit", tree_evidence);
        if let Some(exit) = exit {
            self.publish_launch_exit(&exit);
            self.handoff_launch_exit_after_teardown(&exit);
        }
        finished
    }

    pub(crate) fn clear_tab_agent_identity_explicitly(&self, tab_id: &str, cause: &str) -> bool {
        let cleared = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .map(clear_agent_identity_with_prompt)
                .unwrap_or_default()
        };
        crate::service::launch_readiness::owner_prompt_cleared(tab_id);
        self.finish_agent_identity_clear(
            tab_id,
            cause,
            cleared,
            "explicit-clear",
            "caller_explicitly_cleared_identity",
        )
    }

    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit_during(
        &self,
        tab_id: &str,
        cause: &str,
        shutdown: Option<crate::crash::lifecycle::ShutdownReason>,
    ) -> bool {
        if !self.lifecycle_clear_allowed(tab_id, cause, shutdown.as_ref()) {
            return false;
        }
        if !self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.exit_suspected_at.is_some())
        {
            return self.refuse_lifecycle_identity_clear(
                tab_id,
                cause,
                "provider_exit_marker_not_observed",
            );
        }
        let pty_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pty_id.clone());
        let Some(pty_id) = pty_id else {
            return self.refuse_lifecycle_identity_clear(tab_id, cause, "tab_has_no_live_pty_tree");
        };
        if !self.pty_child_is_live(&pty_id) {
            return self.refuse_lifecycle_identity_clear(tab_id, cause, "tab_pty_child_not_live");
        }
        let Some(tree) = self.observe_provider_process_tree(tab_id, &pty_id) else {
            return self.refuse_lifecycle_identity_clear(
                tab_id,
                cause,
                "provider_capability_unavailable",
            );
        };
        if !self
            .provider_tree_key_for_tab(tab_id, &pty_id)
            .is_some_and(|key| &key == tree.key())
        {
            return self.refuse_lifecycle_identity_clear(
                tab_id,
                cause,
                "tab_identity_changed_during_tree_check",
            );
        }
        if matches!(
            tree.state(),
            crate::service::process_tree::ProviderTreeState::Present
                | crate::service::process_tree::ProviderTreeState::Unknown
        ) {
            return self.refuse_lifecycle_identity_clear(tab_id, cause, tree.evidence());
        }
        if tree.state() == crate::service::process_tree::ProviderTreeState::AbsentBeforeSeen
            && self.lifecycle_clear_has_inflight_launch(tab_id, &pty_id, tree.key())
        {
            return self.refuse_lifecycle_identity_clear(
                tab_id,
                cause,
                "provider_not_seen_before_absence+launch_in_flight",
            );
        }
        let exit_proof =
            crate::service::launch_readiness::LaunchAttempt::current(&self.mux, tab_id, &pty_id)
                .and_then(|attempt| {
                    crate::service::launch_readiness::LaunchExitProof::from_lifecycle_clear(
                        &attempt, &tree,
                    )
                });
        let exited = exit_proof
            .as_ref()
            .and_then(|proof| self.claim_launch_exit(tab_id, Some(&pty_id), None, proof));
        let cleared = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .map(clear_agent_identity_with_prompt)
                .unwrap_or_default()
        };
        crate::service::launch_readiness::owner_prompt_cleared(tab_id);
        let finished = self.finish_agent_identity_clear(
            tab_id,
            cause,
            cleared,
            "lifecycle-clear",
            tree.evidence(),
        );
        if let Some(exited) = exited {
            self.publish_launch_exit(&exited);
            self.handoff_launch_exit_after_teardown(&exited);
        }
        finished
    }

    fn lifecycle_clear_has_inflight_launch(
        &self,
        tab_id: &str,
        pty_id: &str,
        key: &crate::service::process_tree::ProviderTreeKey,
    ) -> bool {
        let tab_in_flight = {
            let mux = self.mux.read();
            let Some(tab) = mux
                .get_tab(tab_id)
                .filter(|tab| tab.pty_id.as_deref() == Some(pty_id))
            else {
                return true;
            };
            tab.agent_booting || launch_stage_before_readiness(tab)
        };
        if tab_in_flight {
            return true;
        }
        let Some(binding_nonce) = key.binding_nonce.as_deref() else {
            return false;
        };
        let attempt = crate::service::launch_readiness::LaunchAttempt {
            tab_id: tab_id.to_string(),
            pty_id: pty_id.to_string(),
            binding_nonce: binding_nonce.to_string(),
        };
        crate::service::launch_readiness::attempt_is_armed(&attempt)
    }

    pub(crate) fn clear_tab_agent_identity_after_exit_with_proof(
        &self,
        tab_id: &str,
        cause: &str,
        proof: &crate::service::launch_readiness::LaunchExitProof,
    ) -> bool {
        if !self.lifecycle_clear_allowed(
            tab_id,
            cause,
            crate::crash::shutdown::active_shutdown().as_ref(),
        ) {
            return false;
        }
        let attempt = proof.attempt();
        if !self.pty_child_is_live(&attempt.pty_id) {
            return self.refuse_lifecycle_identity_clear(
                tab_id,
                cause,
                "finished_marker_pty_child_not_live",
            );
        }
        let key_matches = self
            .provider_tree_key_for_tab(tab_id, &attempt.pty_id)
            .is_some_and(|key| proof.tree_key().is_some_and(|proof_key| &key == proof_key));
        if proof.source_label() != "finished-marker" || !key_matches {
            return self.refuse_lifecycle_identity_clear(
                tab_id,
                cause,
                "finished_marker_tree_identity_mismatch",
            );
        }
        let cleared = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .filter(|tab| crate::service::launch_readiness::attempt_owns_tab(tab, attempt))
                .map(clear_agent_identity_with_prompt)
                .unwrap_or_default()
        };
        crate::service::launch_readiness::owner_prompt_cleared(tab_id);
        self.finish_agent_identity_clear(
            tab_id,
            cause,
            cleared,
            proof.source_label(),
            proof.tree_evidence(),
        )
    }

    pub(crate) fn clear_tab_restored_identity_if_absent(&self, tab_id: &str, cause: &str) -> bool {
        self.clear_tab_agent_identity_for_lifecycle_exit_with_cause(tab_id, cause)
    }

    pub(crate) fn clear_tab_spawn_failed_identity_if_absent(
        &self,
        tab_id: &str,
        cause: &str,
    ) -> bool {
        if !self.lifecycle_clear_allowed(
            tab_id,
            cause,
            crate::crash::shutdown::active_shutdown().as_ref(),
        ) {
            return false;
        }
        let cleared = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .filter(|tab| {
                    tab.boot_stage == Some(crate::service::boot_stage::BootStage::SpawnFailed)
                        && !tab.agent_booting
                })
                .map(clear_agent_identity_with_prompt)
                .unwrap_or_default()
        };
        crate::service::launch_readiness::owner_prompt_cleared(tab_id);
        self.finish_agent_identity_clear(
            tab_id,
            cause,
            cleared,
            "spawn-failed",
            "launch_owner_recorded_spawn_failure",
        )
    }

    fn lifecycle_clear_allowed(
        &self,
        tab_id: &str,
        cause: &str,
        shutdown: Option<&crate::crash::lifecycle::ShutdownReason>,
    ) -> bool {
        if let Some(reason) = shutdown {
            log::info!(
                "[agent-lifecycle] preserving Tab identity during shutdown ({reason:?}): tab={tab_id} cause={cause}"
            );
            return false;
        }
        if self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            (tab.agent_type.is_some() && !tab.launch_session_authenticated())
                || matches!(
                    tab.boot_stage,
                    Some(
                        codeterm_types::pty::BootStage::SpawnFailed
                            | codeterm_types::pty::BootStage::LaunchDeadlineElapsed
                            | codeterm_types::pty::BootStage::LaunchBindingUnconfirmed
                    )
                )
        }) {
            log::info!(
                "[agent-lifecycle] preserving failed launch identity: tab={tab_id} cause={cause}"
            );
            return false;
        }
        if crate::service::codex_rpc::lifecycle::for_tab(tab_id).is_some() {
            log::info!(
                "[agent-lifecycle] preserving sidecar thread identity: tab={tab_id} cause={cause}"
            );
            return false;
        }
        true
    }

    fn refuse_lifecycle_identity_clear(&self, tab_id: &str, cause: &str, evidence: &str) -> bool {
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.exit_suspected_at = None;
        }
        log::warn!(
            "[agent-lifecycle] refused identity clear: tab={tab_id} cause={cause} verdict_source=lifecycle-clear tree_evidence={evidence}"
        );
        false
    }

    fn finish_agent_identity_clear(
        &self,
        tab_id: &str,
        cause: &str,
        identity: ClearedAgentIdentity,
        source: &str,
        tree_evidence: &str,
    ) -> bool {
        if !identity.cleared {
            return false;
        }
        if source != "explicit-clear" {
            if let Some(pty_id) = identity.pty_id.as_deref() {
                self.screen_state.lock().reset_input_modes(pty_id);
            }
        }
        if let Some(identity) = identity.prompt_identity {
            self.resolve_prompt_notifications(tab_id, &identity);
        }
        self.record_lifecycle_end(
            tab_id,
            crate::session::lifecycle::LifecycleTransition::RolledBack {
                reason: cause.to_string(),
            },
        );
        self.invalidate_guarded_agent_input(tab_id);
        self.ext::<std::sync::Arc<crate::service::process_tree::ProviderTreeWitnesses>>()
            .clear_tab(tab_id);
        log::info!(
            "[agent-lifecycle] cleared Tab identity: tab={tab_id} cause={cause} verdict_source={source} tree_evidence={tree_evidence}"
        );
        self.signal_worker_interruption(
            tab_id,
            crate::service::worker_interruption::WorkerInterruptionEvent::ProcessExited,
        );
        self.clear_dead_planner_binding(tab_id);
        self.prompt_detector.lock().clear(tab_id);
        crate::service::structured_prompt::clear_tab(tab_id);
        self.emit_state_changed();
        if matches!(source, "pty-exit" | "lifecycle-clear" | "finished-marker") {
            if let Some(pty_id) = identity.pty_id.as_deref() {
                crate::service::pty_io::submit_transaction::notify_submit_cycle_provider_exit(
                    pty_id,
                );
            }
        }
        true
    }
}

fn launch_stage_before_readiness(tab: &crate::multiplexer::types::Tab) -> bool {
    use codeterm_types::pty::BootStage;

    !tab.restored_from_snapshot
        && matches!(
            tab.boot_stage,
            Some(
                BootStage::PtyStarting
                    | BootStage::PtySpawned
                    | BootStage::ReaderReady
                    | BootStage::AgentLaunching
                    | BootStage::LaunchWritten
                    | BootStage::ProviderReady
                    | BootStage::PreparingRoleInstructions
                    | BootStage::DeliveringInstructions
            )
        )
}
