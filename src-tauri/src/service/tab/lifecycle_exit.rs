use super::ServiceContext;
use crate::service::session_model::clear_runtime_model_evidence;

pub(crate) fn clear_agent_identity_for_lifecycle_exit(
    tab: &mut crate::multiplexer::types::Tab,
) -> bool {
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
        || tab.pending_prompt.is_some();
    if !has_identity {
        return false;
    }
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
    tab.pending_prompt = None;
    tab.exit_suspected_at = None;
    // Provider and model are one identity; clearing only the provider leaves a
    // stale model label attached to the tab.
    clear_runtime_model_evidence(tab);
    crate::service::boot_stage::apply_boot_stage(
        tab,
        codeterm_types::pty::BootStage::ShellReady,
        None,
    );
    true
}

impl ServiceContext {
    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit(&self, tab_id: &str) -> bool {
        self.clear_tab_agent_identity_for_lifecycle_exit_with_cause(
            tab_id,
            "agent identity cleared on lifecycle exit",
        )
    }

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

    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit_during(
        &self,
        tab_id: &str,
        cause: &str,
        shutdown: Option<crate::crash::lifecycle::ShutdownReason>,
    ) -> bool {
        if !self.lifecycle_clear_allowed(tab_id, cause, shutdown.as_ref()) {
            return false;
        }
        let pty_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pty_id.clone());
        let Some(pty_id) = pty_id else {
            return self.refuse_lifecycle_identity_clear(tab_id, cause, "tab_pty_unavailable");
        };
        let Some(tree) = self.observe_provider_process_tree(tab_id, &pty_id, true) else {
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
        if !tree.proves_exit_from_lifecycle_clear() {
            return self.refuse_lifecycle_identity_clear(tab_id, cause, tree.evidence());
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
                .is_some_and(clear_agent_identity_for_lifecycle_exit)
        };
        if let Some(exited) = exited {
            self.publish_launch_exit(&exited);
        }
        self.finish_agent_identity_clear(tab_id, cause, cleared, "lifecycle-clear", tree.evidence())
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
        let key_matches = self
            .provider_tree_key_for_tab(tab_id, &attempt.pty_id)
            .is_some_and(|key| &key == proof.tree_key());
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
                .is_some_and(clear_agent_identity_for_lifecycle_exit)
        };
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
        true
    }

    fn refuse_lifecycle_identity_clear(&self, tab_id: &str, cause: &str, evidence: &str) -> bool {
        log::warn!(
            "[agent-lifecycle] refused identity clear: tab={tab_id} cause={cause} verdict_source=lifecycle-clear tree_evidence={evidence}"
        );
        false
    }

    fn finish_agent_identity_clear(
        &self,
        tab_id: &str,
        cause: &str,
        cleared: bool,
        source: &str,
        tree_evidence: &str,
    ) -> bool {
        if !cleared {
            return false;
        }
        self.record_lifecycle_end(
            tab_id,
            crate::session::lifecycle::LifecycleTransition::RolledBack {
                reason: cause.to_string(),
            },
        );
        self.invalidate_guarded_agent_input(tab_id);
        self.provider_tree_witnesses.clear_tab(tab_id);
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
        true
    }
}
