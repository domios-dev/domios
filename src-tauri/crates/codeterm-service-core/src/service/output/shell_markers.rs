use super::super::ServiceContext;

impl ServiceContext {
    pub(super) fn process_shell_markers_for_pty(&self, pty_id: &str, data: &[u8]) {
        let (tab_id, shell_nonce, binding_nonce) = {
            let mux = self.mux.read();
            let tab_id = mux.tab_id_for_pty(pty_id);
            let (shell_nonce, binding_nonce) = tab_id
                .as_deref()
                .and_then(|tab| mux.get_tab(tab))
                .map(|tab| {
                    (
                        tab.shell_integration_nonce.clone(),
                        tab.session_binding_nonce.clone(),
                    )
                })
                .unwrap_or_default();
            (tab_id, shell_nonce, binding_nonce)
        };
        self.process_shell_markers(pty_id, tab_id.as_deref(), shell_nonce, binding_nonce, data);
    }

    fn publish_shell_command_state(
        &self,
        tab_id: &str,
        pty_id: &str,
        state: crate::multiplexer::types::ShellCommandState,
    ) {
        let changed = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id).is_some_and(|tab| {
                crate::agent::shell_command_state::apply_shell_command_state(tab, pty_id, state)
            })
        };
        if changed {
            self.ui.emit(
                "shell-state-changed",
                serde_json::to_value(
                    serde_json::json!({ "tab_id": tab_id, "pty_id": pty_id, "state": state }),
                )
                .unwrap_or_default(),
            );
        }
    }

    fn process_shell_markers(
        &self,
        pty_id: &str,
        tab_id: Option<&str>,
        nonce: Option<String>,
        binding_nonce: Option<String>,
        data: &[u8],
    ) {
        let (events, command_state) = {
            let mut caps = self.ext::<crate::ShellCapabilityState>().lock();
            let events = caps.feed(pty_id, nonce, data);
            (events, caps.command_state(pty_id))
        };
        let tab_id = tab_id.filter(|id| {
            self.mux
                .read()
                .get_tab(id)
                .is_some_and(|tab| tab.pty_id.as_deref() == Some(pty_id))
        });
        if let Some(tab_id) = tab_id {
            self.publish_shell_command_state(tab_id, pty_id, command_state);
            for event in &events {
                match event {
                    crate::agent::command_detection::CommandLifecycle::Started {
                        command_line,
                        ..
                    } if command_state.activity
                        == crate::multiplexer::types::ShellActivity::Running =>
                    {
                        self.utility_command_started(tab_id, pty_id, command_line.as_deref());
                    }
                    crate::agent::command_detection::CommandLifecycle::Finished { .. } => {
                        if let Err(error) = self.utility_command_finished(tab_id) {
                            log::error!("[utility-timeout] command completion failed: {error}");
                        }
                    }
                    crate::agent::command_detection::CommandLifecycle::CwdChanged { cwd } => {
                        self.update_tab_cwd(tab_id, cwd.clone());
                    }
                    _ => {}
                }
            }
        }
        if events.is_empty() {
            return;
        }
        for event in &events {
            crate::service::shell_exec::observe_command(pty_id, event);
        }
        let Some(tab_id) = tab_id else {
            return;
        };
        let launch_attempt =
            binding_nonce.map(
                |binding_nonce| crate::service::launch_readiness::LaunchAttempt {
                    tab_id: tab_id.to_owned(),
                    pty_id: pty_id.to_owned(),
                    binding_nonce,
                },
            );
        for event in &events {
            if let crate::agent::command_detection::CommandLifecycle::Finished {
                exit_code,
                command_line,
                ..
            } = event
            {
                let cron_launch_pending =
                    self.run_watcher.cron_launch_exit_eligibility(self, pty_id) == Some(false)
                        || launch_attempt
                            .as_ref()
                            .is_some_and(crate::service::launch_readiness::cron_launch_is_pending);
                let eligible = launch_attempt.as_ref().filter(|attempt| {
                    !cron_launch_pending
                        && self.finished_marker_owns_launch(attempt, command_line.as_deref())
                });
                let tree = eligible
                    .filter(|_| self.pty_child_is_live(pty_id))
                    .and_then(|_| self.observe_provider_process_tree(tab_id, pty_id));
                let proof = eligible.zip(tree.as_ref()).and_then(|(attempt, tree)| {
                    self.command_finished_proves_provider_exit(attempt, event, tree)
                });
                if cron_launch_pending {
                    log::info!(
                        "[launch-readiness] ignored Finished marker before cron provider launch tab={tab_id} pty={pty_id}"
                    );
                }
                let proof = match proof {
                    Some(proof)
                        if proof.is_deferred()
                            && crate::service::launch_readiness::suspect_exit(
                                tab_id,
                                Some(pty_id),
                                *exit_code,
                                &proof,
                            ) =>
                    {
                        log::info!(
                            "[launch-readiness] deferred verdict_source=finished-marker tree_evidence={} until binding or startup deadline tab={tab_id} pty={pty_id}",
                            proof.tree_evidence()
                        );
                        None
                    }
                    other => other,
                };
                if let Some(proof) = proof {
                    self.handle_proven_provider_exit(tab_id, pty_id, *exit_code, &proof);
                } else if tree.is_none() && eligible.is_some() {
                    log::info!(
                        "[launch-readiness] refused verdict_source=finished-marker tree_evidence=provider_tree_unavailable tab={tab_id} pty={pty_id}"
                    );
                }
            }
            self.ui.emit(
                "shell-command-changed",
                serde_json::to_value(serde_json::json!({ "tab_id": tab_id, "event": event }))
                    .unwrap_or_default(),
            );
        }
    }
}

impl ServiceContext {
    /// Settle a provider exit that is proven for this launch: record the
    /// verdict, tear the agent identity down and hand off to recovery.
    pub(crate) fn handle_proven_provider_exit(
        &self,
        tab_id: &str,
        pty_id: &str,
        exit_code: Option<i32>,
        proof: &crate::service::launch_readiness::LaunchExitProof,
    ) {
        let ga_snapshot = {
            self.mux
                .read()
                .get_tab(tab_id)
                .filter(|tab| {
                    crate::service::launch_readiness::attempt_owns_tab(tab, proof.attempt())
                })
                .cloned()
        };
        if ga_snapshot.is_none() {
            return;
        }
        let ga_tree_key = self.provider_tree_key_for_tab(tab_id, pty_id);
        let general_exit = ga_snapshot
            .as_ref()
            .is_some_and(crate::service::general_agent::is_general_agent);
        let captured_exit = self.capture_exit_tab_observation(tab_id);
        self.observe_agent_startup_launch_command_finished(tab_id, pty_id, exit_code);
        let exited = self.claim_launch_exit(tab_id, Some(pty_id), exit_code, proof);
        let cron_run_reaped = self.run_watcher.fail_cron_on_process_exit(
            self,
            pty_id,
            "provider process exited to a surviving shell".into(),
        );
        let note_surviving_shell_exit = !cron_run_reaped
            && !general_exit
            && !crate::crash::shutdown::is_shutting_down()
            && self.note_provider_exit_in_surviving_shell(tab_id, proof);
        if !note_surviving_shell_exit && !cron_run_reaped {
            self.clear_tab_agent_identity_after_exit_with_proof(
                tab_id,
                "provider process exited to a surviving shell",
                proof,
            );
        }
        if let Some(exited) = exited {
            self.publish_launch_exit(&exited);
            if !note_surviving_shell_exit && !general_exit {
                self.handoff_launch_exit_after_teardown(&exited);
            }
        }
        if general_exit {
            log::info!("[agent-lifecycle] provider exited to a surviving shell: tab={tab_id}");
        }
        self.observe_captured_tab_exit(tab_id, captured_exit);
        if let (Some(tab), Some(key)) = (ga_snapshot, ga_tree_key) {
            self.general_agent_provider_exit_event(&tab, &key);
        }
        if note_surviving_shell_exit && !general_exit {
            self.notify_surviving_shell_restart_required(tab_id, pty_id);
        }
    }
}
