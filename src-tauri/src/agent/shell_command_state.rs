use crate::multiplexer::types::{ShellActivity, ShellCommandState};
pub use codeterm_agent_detect::shell_command_state::ShellCommandStateModel;

pub fn apply_shell_command_state(
    tab: &mut crate::multiplexer::types::Tab,
    pty_id: &str,
    state: ShellCommandState,
) -> bool {
    if !tab.is_utility_shell()
        || tab.pty_id.as_deref() != Some(pty_id)
        || tab.shell_command_state == state
    {
        return false;
    }
    if tab.utility_command_pending {
        if state.activity != ShellActivity::Running && state.last_exit_code.is_none() {
            return false;
        }
        tab.utility_command_pending = false;
    }
    tab.shell_command_state = state;
    true
}

#[cfg(test)]
mod tests;
