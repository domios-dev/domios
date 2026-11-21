use crate::multiplexer::types::{ShellActivity, Tab};
use crate::service::ServiceContext;

impl ServiceContext {
    pub(super) fn capture_running_shells(&self, initiating_tab: Option<&str>) {
        let mut mux = self.mux.write();
        let interrupted: Vec<_> = mux
            .all_tabs()
            .filter(|tab| Some(tab.id.as_str()) != initiating_tab)
            .filter_map(|tab| {
                self.shutdown_shell_command(tab)
                    .map(|command| (tab.id.clone(), command))
            })
            .collect();
        for (tab_id, command) in interrupted {
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                let restore = tab.close_restore.get_or_insert_with(Default::default);
                restore.command = Some(command);
                restore.stopped = true;
            }
        }
    }

    pub(super) fn shutdown_shell_command(&self, tab: &Tab) -> Option<String> {
        if !tab.is_utility_shell()
            || tab.agent_booting
            || tab.shell_command_state.activity != ShellActivity::Running
        {
            return None;
        }
        let command = self.running_command_on_pty(&tab.id, tab.pty_id.as_deref()?)?;
        codeterm_wire::shell_command::validate(&command).ok()?;
        Some(command)
    }
}

#[cfg(test)]
mod tests;
