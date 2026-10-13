use super::super::ServiceContext;

impl ServiceContext {
    pub(super) fn process_shell_markers_for_pty(&self, pty_id: &str, data: &[u8]) {
        let (tab_id, nonce) = {
            let mux = self.mux.read();
            let tab_id = mux.tab_id_for_pty(pty_id);
            let nonce = tab_id
                .as_ref()
                .and_then(|tab| mux.get_tab(tab))
                .and_then(|tab| tab.shell_integration_nonce.clone());
            (tab_id, nonce)
        };
        self.process_shell_markers(pty_id, tab_id.as_deref(), nonce, data);
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
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "shell-state-changed",
                serde_json::json!({ "tab_id": tab_id, "pty_id": pty_id, "state": state }),
            );
        }
    }

    pub(super) fn process_shell_markers(
        &self,
        pty_id: &str,
        tab_id: Option<&str>,
        nonce: Option<String>,
        data: &[u8],
    ) {
        let (events, command_state) = {
            let mut caps = self.shell_caps.lock();
            let events = caps.feed(pty_id, nonce, data);
            (events, caps.command_state(pty_id))
        };
        if let Some(tab_id) = tab_id {
            self.publish_shell_command_state(tab_id, pty_id, command_state);
            for event in &events {
                if let crate::agent::command_detection::CommandLifecycle::Started {
                    command_line,
                    ..
                } = event
                {
                    self.utility_command_started(tab_id, command_line.as_deref());
                }
                if let crate::agent::command_detection::CommandLifecycle::CwdChanged { cwd } = event
                {
                    self.update_tab_cwd(tab_id, cwd.clone());
                }
            }
        }
        if events.is_empty() {
            return;
        }
        for event in &events {
            if let crate::agent::command_detection::CommandLifecycle::Finished {
                exit_code, ..
            } = event
            {
                crate::service::shell_exec::publish(
                    pty_id,
                    crate::service::shell_exec::ExecCompletion::CommandFinished {
                        exit_code: *exit_code,
                    },
                );
            }
        }
        let Some(tab_id) = tab_id else {
            return;
        };
        let launch_attempt =
            crate::service::launch_readiness::LaunchAttempt::current(&self.mux, tab_id, pty_id);
        let cron_launch_pending = launch_attempt
            .as_ref()
            .is_some_and(crate::service::launch_readiness::cron_launch_is_pending);
        let tree_observation = (!cron_launch_pending && self.pty_child_is_live(pty_id))
            .then(|| self.observe_provider_process_tree(tab_id, pty_id))
            .flatten();
        for event in &events {
            if let crate::agent::command_detection::CommandLifecycle::Finished {
                exit_code, ..
            } = event
            {
                let cron_launch_pending =
                    self.run_watcher.cron_launch_exit_eligibility(self, pty_id) == Some(false)
                        || launch_attempt
                            .as_ref()
                            .is_some_and(crate::service::launch_readiness::cron_launch_is_pending);
                let tree = tree_observation.as_ref();
                let proof = if cron_launch_pending {
                    None
                } else {
                    tree.and_then(|tree| {
                        self.command_finished_proves_provider_exit(tab_id, pty_id, event, tree)
                    })
                };
                if cron_launch_pending {
                    log::info!(
                        "[launch-readiness] ignored Finished marker before cron provider launch tab={tab_id} pty={pty_id}"
                    );
                }
                if let Some(sidecar) = crate::service::codex_rpc::lifecycle::for_tab(tab_id)
                    .filter(|_| proof.is_some())
                {
                    crate::service::codex_rpc::recovery::try_start(
                        self,
                        tab_id,
                        &sidecar.launch_nonce,
                        "remote TUI exited to shell",
                    );
                    crate::api::events::emit_everywhere(
                        &self.app_handle,
                        "shell-command-changed",
                        serde_json::json!({ "tab_id": tab_id, "event": event }),
                    );
                    continue;
                }
                if let Some(proof) = proof {
                    let ga_snapshot = { self.mux.read().get_tab(tab_id).cloned() };
                    let ga_tree_key = self.provider_tree_key_for_tab(tab_id, pty_id);
                    let general_exit = ga_snapshot
                        .as_ref()
                        .is_some_and(crate::service::general_agent::is_general_agent);
                    let captured_exit = self.capture_exit_tab_observation(tab_id);
                    self.observe_agent_startup_launch_command_finished(tab_id, pty_id, *exit_code);
                    let exited = self.claim_launch_exit(tab_id, Some(pty_id), *exit_code, &proof);
                    let cron_run_reaped = self.run_watcher.fail_cron_on_process_exit(
                        self,
                        pty_id,
                        "provider process exited to a surviving shell".into(),
                    );
                    let note_surviving_shell_exit = !cron_run_reaped
                        && !general_exit
                        && !crate::crash::shutdown::is_shutting_down()
                        && self.note_provider_exit_in_surviving_shell(tab_id, &proof);
                    if !note_surviving_shell_exit && !cron_run_reaped {
                        self.clear_tab_agent_identity_after_exit_with_proof(
                            tab_id,
                            "provider process exited to a surviving shell",
                            &proof,
                        );
                    }
                    if let Some(exited) = exited {
                        self.publish_launch_exit(&exited);
                        if !note_surviving_shell_exit && !general_exit {
                            self.handoff_launch_exit_after_teardown(&exited);
                        }
                    }
                    if general_exit {
                        log::info!(
                            "[agent-lifecycle] provider exited to a surviving shell: tab={tab_id}"
                        );
                    }
                    self.observe_captured_tab_exit(tab_id, captured_exit);
                    if let (Some(tab), Some(key)) = (ga_snapshot, ga_tree_key) {
                        self.general_agent_provider_exit_event(&tab, &key);
                    }
                    if note_surviving_shell_exit && !general_exit {
                        self.notify_surviving_shell_restart_required(tab_id, pty_id);
                    }
                } else if tree.is_none() && !cron_launch_pending {
                    log::info!(
                        "[launch-readiness] refused verdict_source=finished-marker tree_evidence=provider_tree_unavailable tab={tab_id} pty={pty_id}"
                    );
                }
            }
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "shell-command-changed",
                serde_json::json!({ "tab_id": tab_id, "event": event }),
            );
        }
    }
}
