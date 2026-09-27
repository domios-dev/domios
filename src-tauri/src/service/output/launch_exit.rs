use crate::agent::command_detection::CommandLifecycle;
use crate::service::launch_readiness::{LaunchAttempt, LaunchExitProof};
use crate::service::process_tree::{ProviderTreeKey, ProviderTreeObservation};

pub(super) fn finished_event_proves_provider_exit(
    event: &CommandLifecycle,
    tree: &ProviderTreeObservation,
) -> bool {
    matches!(event, CommandLifecycle::Finished { .. }) && tree.proves_exit_from_finished_marker()
}

impl super::super::ServiceContext {
    pub(crate) fn provider_tree_key_for_tab(
        &self,
        tab_id: &str,
        pty_id: &str,
    ) -> Option<ProviderTreeKey> {
        let (provider_id, binding_nonce) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            if tab.pty_id.as_deref() != Some(pty_id) {
                return None;
            }
            (tab.agent_type.clone()?, tab.session_binding_nonce.clone())
        };
        let executable_name = self
            .agent_registry
            .with_provider(&provider_id, |provider| provider.binary_name().to_string())?;
        Some(ProviderTreeKey {
            tab_id: tab_id.to_string(),
            pty_id: pty_id.to_string(),
            binding_nonce,
            executable_name,
        })
    }

    pub(crate) fn observe_provider_process_tree(
        &self,
        tab_id: &str,
        pty_id: &str,
        force: bool,
    ) -> Option<ProviderTreeObservation> {
        let Some(key) = self.provider_tree_key_for_tab(tab_id, pty_id) else {
            return None;
        };
        if !force && self.provider_tree_witnesses.has_seen(&key) {
            return None;
        }
        let root_pid = {
            let pty = self.pty.lock();
            let Some(session) = pty.get_session(pty_id) else {
                return Some(self.provider_tree_witnesses.unavailable(&key));
            };
            let Ok(child) = session.child.lock() else {
                return Some(self.provider_tree_witnesses.unavailable(&key));
            };
            child.process_id()
        };
        let Some(root_pid) = root_pid else {
            return Some(self.provider_tree_witnesses.unavailable(&key));
        };
        Some(
            self.provider_tree_witnesses
                .observe(&key, root_pid, &key.executable_name),
        )
    }

    pub(super) fn command_finished_proves_provider_exit(
        &self,
        tab_id: &str,
        pty_id: &str,
        event: &CommandLifecycle,
        tree: &ProviderTreeObservation,
    ) -> Option<LaunchExitProof> {
        if !matches!(event, CommandLifecycle::Finished { .. })
            || !finished_event_proves_provider_exit(event, tree)
        {
            return None;
        }
        let attempt = LaunchAttempt::current(&self.mux, tab_id, pty_id)?;
        LaunchExitProof::from_finished_marker(&attempt, tree)
    }
}

#[cfg(test)]
mod tests {
    use super::finished_event_proves_provider_exit;
    use crate::agent::command_detection::CommandLifecycle;
    use crate::service::process_tree::{
        ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeKey, ProviderTreeObservation,
        ProviderTreeState, ProviderTreeWitnesses,
    };

    fn finished(command_line: Option<&str>) -> CommandLifecycle {
        CommandLifecycle::Finished {
            command_line: command_line.map(str::to_string),
            exit_code: Some(3),
            cwd: None,
        }
    }

    fn tree(state: ProviderTreeState) -> ProviderTreeObservation {
        let witnesses = ProviderTreeWitnesses::default();
        let key = ProviderTreeKey {
            tab_id: "tab-finished-marker".into(),
            pty_id: "pty-finished-marker".into(),
            binding_nonce: Some("nonce-finished-marker".into()),
            executable_name: "opencode".into(),
        };
        if matches!(
            state,
            ProviderTreeState::AbsentOnceAfterSeen | ProviderTreeState::GoneAfterSeen
        ) {
            witnesses.record_for_test(
                &key,
                ProviderProcessTreeSnapshot::Present(
                    [ProcessIdentity {
                        pid: 52_520,
                        start_token: "test-opencode-start".into(),
                    }]
                    .into_iter()
                    .collect(),
                ),
            );
        }
        let target_absences = match state {
            ProviderTreeState::AbsentOnceAfterSeen => 1,
            ProviderTreeState::GoneAfterSeen => 2,
            _ => 0,
        };
        if target_absences > 0 {
            for _ in 0..target_absences {
                let observation =
                    witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
                if observation.state() == state {
                    return observation;
                }
            }
        }
        let snapshot = match state {
            ProviderTreeState::Present => ProviderProcessTreeSnapshot::Present(
                [ProcessIdentity {
                    pid: 52_520,
                    start_token: "test-opencode-start".into(),
                }]
                .into_iter()
                .collect(),
            ),
            ProviderTreeState::Unknown => ProviderProcessTreeSnapshot::Unknown,
            ProviderTreeState::AbsentBeforeSeen => ProviderProcessTreeSnapshot::Absent,
            ProviderTreeState::AbsentOnceAfterSeen | ProviderTreeState::GoneAfterSeen => {
                unreachable!("the target absence was returned above")
            }
        };
        witnesses.record_for_test(&key, snapshot)
    }

    #[test]
    fn finished_marker_exit_uses_process_tree_for_real_shell_marker_shapes() {
        let powershell = finished(None);
        let unix_spawn_wrapper = finished(Some("/bin/bash /tmp/codeterm-spawn-0123.sh"));
        let gone = tree(ProviderTreeState::GoneAfterSeen);

        for marker in [&powershell, &unix_spawn_wrapper] {
            assert!(finished_event_proves_provider_exit(marker, &gone));
            assert!(!finished_event_proves_provider_exit(
                marker,
                &tree(ProviderTreeState::Present)
            ));
            assert!(!finished_event_proves_provider_exit(
                marker,
                &tree(ProviderTreeState::AbsentOnceAfterSeen)
            ));
            assert!(!finished_event_proves_provider_exit(
                marker,
                &tree(ProviderTreeState::AbsentBeforeSeen)
            ));
        }
        assert!(!finished_event_proves_provider_exit(
            &CommandLifecycle::Started {
                command_line: None,
                cwd: None,
            },
            &gone,
        ));
    }
}
