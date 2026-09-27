use crate::agent::command_detection::CommandLifecycle;

pub(super) fn finished_event_proves_provider_exit(
    event: &CommandLifecycle,
    provider_process_may_be_alive: bool,
) -> bool {
    matches!(event, CommandLifecycle::Finished { .. }) && !provider_process_may_be_alive
}

impl super::super::ServiceContext {
    pub(super) fn command_finished_proves_provider_exit(
        &self,
        tab_id: &str,
        pty_id: &str,
        event: &CommandLifecycle,
    ) -> bool {
        let provider_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .filter(|tab| tab.pty_id.as_deref() == Some(pty_id))
            .and_then(|tab| tab.agent_type.clone());
        let Some(provider_id) = provider_id else {
            return false;
        };
        let Some(binary_name) = self
            .agent_registry
            .with_provider(&provider_id, |provider| provider.binary_name().to_string())
        else {
            return false;
        };
        let root_pid = {
            let pty = self.pty.lock();
            let Some(session) = pty.get_session(pty_id) else {
                return false;
            };
            let Ok(child) = session.child.lock() else {
                return false;
            };
            child.process_id()
        };
        let provider_process_may_be_alive = root_pid.is_none_or(|root| {
            super::super::process_tree::may_have_named_process(root, &binary_name)
        });
        finished_event_proves_provider_exit(event, provider_process_may_be_alive)
    }
}

#[cfg(test)]
mod tests {
    use super::finished_event_proves_provider_exit;
    use crate::agent::command_detection::CommandLifecycle;

    fn finished(command_line: Option<&str>) -> CommandLifecycle {
        CommandLifecycle::Finished {
            command_line: command_line.map(str::to_string),
            exit_code: Some(3),
            cwd: None,
        }
    }

    #[test]
    fn finished_marker_exit_uses_process_tree_for_real_shell_marker_shapes() {
        let powershell = finished(None);
        let unix_spawn_wrapper = finished(Some("/bin/bash /tmp/codeterm-spawn-0123.sh"));

        for marker in [&powershell, &unix_spawn_wrapper] {
            assert!(finished_event_proves_provider_exit(marker, false));
            assert!(!finished_event_proves_provider_exit(marker, true));
        }
        assert!(!finished_event_proves_provider_exit(
            &CommandLifecycle::Started {
                command_line: None,
                cwd: None,
            },
            false,
        ));
    }
}
