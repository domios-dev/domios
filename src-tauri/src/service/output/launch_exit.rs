use crate::agent::command_detection::CommandLifecycle;
use crate::service::launch_readiness::{LaunchAttempt, LaunchExitProof};
use crate::service::process_tree::{ProviderTreeKey, ProviderTreeObservation, ProviderTreeState};

pub(super) fn finished_event_proves_provider_exit(
    event: &CommandLifecycle,
    tree: &ProviderTreeObservation,
    may_have_named_process: Option<bool>,
) -> bool {
    match event {
        CommandLifecycle::Finished { .. } => {
            let finished_event_proves_exit_before_first_seen =
                finished_marker_has_explicit_exit_evidence(event);
            tree.proves_exit_from_finished_marker()
                || (finished_event_proves_exit_before_first_seen
                    && tree.state() == ProviderTreeState::AbsentBeforeSeen)
                || (tree.state() == ProviderTreeState::AbsentBeforeSeen
                    && tree.has_distinct_absence_observations()
                    && may_have_named_process == Some(false))
        }
        CommandLifecycle::Started { .. } | CommandLifecycle::CwdChanged { .. } => false,
    }
}

fn finished_marker_has_explicit_exit_evidence(event: &CommandLifecycle) -> bool {
    let CommandLifecycle::Finished {
        command_line,
        exit_code,
        ..
    } = event
    else {
        return false;
    };
    exit_code.is_some_and(|code| code != 0)
        || (exit_code.is_some()
            && command_line
                .as_deref()
                .is_some_and(|line| !line.trim().is_empty()))
}

impl super::super::ServiceContext {
    pub(crate) fn provider_tree_key_for_tab(
        &self,
        tab_id: &str,
        pty_id: &str,
    ) -> Option<ProviderTreeKey> {
        if self.tab_chat_backing(tab_id).is_some() {
            return None;
        }
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
    ) -> Option<ProviderTreeObservation> {
        let key = self.provider_tree_key_for_tab(tab_id, pty_id)?;
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

    fn provider_may_have_named_process(&self, key: &ProviderTreeKey) -> Option<bool> {
        let root_pid = {
            let pty = self.pty.lock();
            let session = pty.get_session(&key.pty_id)?;
            let child = session.child.lock().ok()?;
            child.process_id()?
        };
        Some(crate::service::process_tree::may_have_named_process(
            root_pid,
            &key.executable_name,
        ))
    }

    pub(super) fn command_finished_proves_provider_exit(
        &self,
        tab_id: &str,
        pty_id: &str,
        event: &CommandLifecycle,
        tree: &ProviderTreeObservation,
    ) -> Option<LaunchExitProof> {
        let CommandLifecycle::Finished {
            command_line,
            exit_code,
            ..
        } = event
        else {
            return None;
        };
        let finished_event_proves_exit_before_first_seen =
            finished_marker_has_explicit_exit_evidence(event);
        let may_have_named_process = (tree.state() == ProviderTreeState::AbsentBeforeSeen
            && tree.has_distinct_absence_observations()
            && !finished_event_proves_exit_before_first_seen)
            .then(|| self.provider_may_have_named_process(tree.key()))
            .flatten();
        if !finished_event_proves_provider_exit(event, tree, may_have_named_process) {
            let fallback_evidence = match may_have_named_process {
                Some(true) => "+may_have_named_process_present_or_unknown",
                Some(false) => "+may_have_named_process_absent",
                None if tree.has_distinct_absence_observations() => "+tab_tree_scan_unavailable",
                None => "",
            };
            log::info!(
                "[launch-readiness] refused verdict_source=finished-marker tree_evidence={}{} tab={} pty={}",
                tree.evidence(),
                fallback_evidence,
                tab_id,
                pty_id,
            );
            return None;
        }
        let Some(attempt) = LaunchAttempt::current(&self.mux, tab_id, pty_id) else {
            log::info!(
                "[launch-readiness] refused verdict_source=finished-marker tree_evidence={} attempt_unavailable tab={} pty={}",
                tree.evidence(),
                tab_id,
                pty_id,
            );
            return None;
        };
        let proof = LaunchExitProof::from_finished_marker(
            &attempt,
            tree,
            *exit_code,
            finished_event_proves_exit_before_first_seen,
            may_have_named_process,
        );
        if proof.is_none() {
            log::info!(
                "[launch-readiness] refused verdict_source=finished-marker tree_evidence={} attempt_mismatch tab={} pty={}",
                tree.evidence(),
                tab_id,
                pty_id,
            );
        }
        proof
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

    fn finished(command_line: Option<&str>, exit_code: Option<i32>) -> CommandLifecycle {
        CommandLifecycle::Finished {
            command_line: command_line.map(str::to_string),
            exit_code,
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
        if matches!(state, ProviderTreeState::GoneAfterSeen) {
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
        if state == ProviderTreeState::GoneAfterSeen {
            return witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
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
            ProviderTreeState::GoneAfterSeen => {
                unreachable!("the target absence was returned above")
            }
        };
        witnesses.record_for_test(&key, snapshot)
    }

    fn absent_twice() -> ProviderTreeObservation {
        let witnesses = ProviderTreeWitnesses::default();
        let key = ProviderTreeKey {
            tab_id: "tab-finished-marker".into(),
            pty_id: "pty-finished-marker".into(),
            binding_nonce: Some("nonce-finished-marker".into()),
            executable_name: "opencode".into(),
        };
        witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
        witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent)
    }

    #[test]
    fn finished_marker_exit_uses_process_tree_for_real_shell_marker_shapes() {
        let powershell = finished(None, None);
        let unix_spawn_wrapper = finished(Some("/bin/bash /tmp/codeterm-spawn-0123.sh"), None);
        let failed_command = finished(None, Some(3));
        let gone = tree(ProviderTreeState::GoneAfterSeen);

        for marker in [&powershell, &unix_spawn_wrapper] {
            assert!(finished_event_proves_provider_exit(marker, &gone, None));
            assert!(!finished_event_proves_provider_exit(
                marker,
                &tree(ProviderTreeState::Present),
                None
            ));
            assert!(!finished_event_proves_provider_exit(
                marker,
                &tree(ProviderTreeState::AbsentBeforeSeen),
                None
            ));
        }
        assert!(finished_event_proves_provider_exit(
            &failed_command,
            &tree(ProviderTreeState::AbsentBeforeSeen),
            None
        ));
        assert!(!finished_event_proves_provider_exit(
            &failed_command,
            &tree(ProviderTreeState::Present),
            None
        ));
        assert!(!finished_event_proves_provider_exit(
            &failed_command,
            &tree(ProviderTreeState::Unknown),
            None
        ));
        assert!(!finished_event_proves_provider_exit(
            &finished(None, Some(0)),
            &tree(ProviderTreeState::AbsentBeforeSeen),
            None
        ));
        assert!(finished_event_proves_provider_exit(
            &finished(Some("opencode"), Some(0)),
            &tree(ProviderTreeState::AbsentBeforeSeen),
            None
        ));
        let absent_twice = absent_twice();
        assert!(!finished_event_proves_provider_exit(
            &powershell,
            &absent_twice,
            None
        ));
        assert!(!finished_event_proves_provider_exit(
            &powershell,
            &absent_twice,
            Some(true)
        ));
        assert!(finished_event_proves_provider_exit(
            &powershell,
            &absent_twice,
            Some(false)
        ));
        assert!(!finished_event_proves_provider_exit(
            &CommandLifecycle::Started {
                command_line: None,
                cwd: None,
            },
            &gone,
            None,
        ));
    }
}
