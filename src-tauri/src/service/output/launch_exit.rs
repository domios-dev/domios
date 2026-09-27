pub(super) fn finished_event_proves_provider_exit(
    command_line: Option<&str>,
    binary_name: &str,
    provider_process_may_be_alive: bool,
) -> bool {
    command_line.is_some_and(|command| command_mentions_binary(command, binary_name))
        && !provider_process_may_be_alive
}

fn command_mentions_binary(command: &str, binary_name: &str) -> bool {
    fn normalize(name: &str) -> &str {
        let base = name.rsplit(['\\', '/']).next().unwrap_or(name);
        base.strip_suffix(".exe")
            .or_else(|| base.strip_suffix(".EXE"))
            .unwrap_or(base)
    }
    let expected = normalize(binary_name);
    command
        .split(|character: char| {
            character.is_whitespace()
                || matches!(character, ';' | '&' | '|' | '{' | '}' | '(' | ')')
        })
        .any(|token| {
            let token = token.trim_matches(['\'', '"', '`']);
            normalize(token).eq_ignore_ascii_case(expected)
        })
}

impl super::super::ServiceContext {
    pub(super) fn command_finished_proves_provider_exit(
        &self,
        tab_id: &str,
        pty_id: &str,
        command_line: Option<&str>,
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
        if !command_line.is_some_and(|command| command_mentions_binary(command, &binary_name)) {
            return false;
        }
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
        finished_event_proves_provider_exit(
            command_line,
            &binary_name,
            provider_process_may_be_alive,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::finished_event_proves_provider_exit;

    #[test]
    fn opencode_staging_wrapper_finishes_do_not_claim_a_live_provider_exit() {
        let wrapper = "Set-Location -LiteralPath D:/project -ErrorAction Stop; New-Item -ItemType Directory -Force -Path '.codeterm' | Out-Null; Copy-Item -LiteralPath C:/tmp/task.md -Destination .codeterm/task.md -Force; $env:OPENCODE_CONFIG_CONTENT = '{}'; try { opencode --session ses_saved -m big-pickle } finally { Remove-Item -LiteralPath .codeterm/task.md }";
        for staging in [
            "Set-Location -LiteralPath D:/project -ErrorAction Stop",
            "New-Item -ItemType Directory -Force -Path '.codeterm' | Out-Null",
            "Copy-Item -LiteralPath C:/tmp/task.md -Destination .codeterm/task.md -Force",
        ] {
            assert!(!finished_event_proves_provider_exit(
                Some(staging),
                "opencode",
                false,
            ));
        }
        assert!(!finished_event_proves_provider_exit(
            Some(wrapper),
            "opencode",
            true,
        ));
        assert!(finished_event_proves_provider_exit(
            Some(wrapper),
            "opencode.exe",
            false,
        ));
    }
}
