use super::super::ServiceContext;

impl ServiceContext {
    pub(super) fn process_shell_markers(
        &self,
        pty_id: &str,
        tab_id: Option<&str>,
        nonce: Option<String>,
        data: &[u8],
    ) {
        let events = self.shell_caps.lock().feed(pty_id, nonce, data);
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
        let tree_observation = self.observe_provider_process_tree(tab_id, pty_id);
        for event in &events {
            if let crate::agent::command_detection::CommandLifecycle::Finished {
                exit_code, ..
            } = event
            {
                let tree = tree_observation.as_ref();
                let proof = tree.and_then(|tree| {
                    self.command_finished_proves_provider_exit(tab_id, pty_id, event, tree)
                });
                if let Some(proof) = proof {
                    self.observe_agent_startup_launch_command_finished(tab_id, pty_id, *exit_code);
                    let exited = self.claim_launch_exit(tab_id, Some(pty_id), *exit_code, &proof);
                    let cron_run_reaped = self.run_watcher.fail_cron_on_process_exit(
                        self,
                        pty_id,
                        "provider process exited to a surviving shell".into(),
                    );
                    let note_surviving_shell_exit = !cron_run_reaped
                        && !crate::crash::shutdown::is_shutting_down()
                        && self.note_provider_exit_in_surviving_shell(tab_id);
                    if !note_surviving_shell_exit && !cron_run_reaped {
                        self.clear_tab_agent_identity_after_exit_with_proof(
                            tab_id,
                            "provider process exited to a surviving shell",
                            &proof,
                        );
                    }
                    if let Some(exited) = exited {
                        self.publish_launch_exit(&exited);
                        if !note_surviving_shell_exit {
                            self.handoff_launch_exit_after_teardown(&exited);
                        }
                    }
                    if note_surviving_shell_exit {
                        self.notify_surviving_shell_restart_required(tab_id, pty_id);
                    }
                } else if tree.is_none() {
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
