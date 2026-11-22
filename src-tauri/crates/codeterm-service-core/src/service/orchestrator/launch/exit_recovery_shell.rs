impl crate::service::ServiceContext {
    pub fn recovery_shell_has_idle_prompt(&self, pty_id: &str) -> bool {
        self.ext::<crate::ShellCapabilityState>()
            .lock()
            .prompt_idle_after_command(pty_id)
    }
}

pub(super) fn exited_provider_recovery_has_shell_ready_evidence(
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
    reused_live_pty: bool,
    prompt_idle_after_command: impl FnOnce() -> bool,
) -> bool {
    if !reused_live_pty {
        return false;
    }
    match origin {
        crate::service::launch_diagnostics::AgentLaunchOrigin::AutomaticExitRecovery => true,
        crate::service::launch_diagnostics::AgentLaunchOrigin::ExitRecovery => {
            prompt_idle_after_command()
        }
        _ => false,
    }
}
