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
        let finished_marker = events.iter().any(|event| {
            matches!(
                event,
                crate::agent::command_detection::CommandLifecycle::Finished { .. }
            )
        });
        let tree_observation = tab_id
            .and_then(|tab_id| self.observe_provider_process_tree(tab_id, pty_id, finished_marker));
        if events.is_empty() {
            return;
        }
        let Some(tab_id) = tab_id else {
            return;
        };
        for event in &events {
            if let crate::agent::command_detection::CommandLifecycle::Finished {
                exit_code, ..
            } = event
            {
                let tree = self
                    .observe_provider_process_tree(tab_id, pty_id, true)
                    .or_else(|| tree_observation.clone());
                let proof = tree.as_ref().and_then(|tree| {
                    self.command_finished_proves_provider_exit(tab_id, pty_id, event, tree)
                });
                if let Some(proof) = proof {
                    self.observe_agent_startup_launch_command_finished(tab_id, pty_id, *exit_code);
                    let exited = self.claim_launch_exit(tab_id, Some(pty_id), *exit_code, &proof);
                    self.clear_tab_agent_identity_after_exit_with_proof(
                        tab_id,
                        "provider process exited to a surviving shell",
                        &proof,
                    );
                    if let Some(exited) = exited {
                        self.publish_launch_exit(&exited);
                    }
                } else {
                    let evidence = tree
                        .as_ref()
                        .map(|tree| tree.evidence())
                        .unwrap_or("launch_attempt_unavailable");
                    log::info!(
                        "[launch-readiness] refused verdict_source=finished-marker tree_evidence={evidence} tab={tab_id} pty={pty_id}"
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
