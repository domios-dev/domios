use crate::service::MuxEmitCause;
use std::time::Duration;

use serde::Serialize;

use super::ServiceContext;

mod submission;
pub use submission::ShellCommandSubmission;

#[derive(Debug, Serialize)]
pub struct ShellRestartOutcome {
    pub tab_id: String,
    pub old_pty_id: Option<String>,
    pub new_pty_id: String,
    pub cwd: Option<String>,
    pub command_delivered: bool,
    pub command_submission: Option<ShellCommandSubmission>,
}

impl ServiceContext {
    pub fn restart_shell_tab(
        &self,
        tab_id: &str,
        cwd: Option<String>,
        command: Option<&str>,
    ) -> Result<ShellRestartOutcome, String> {
        if let Some(command) = command {
            codeterm_wire::shell_command::validate(command)?;
        }
        let (old_pty_id, current_cwd, agent_type, agent_member) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {tab_id} not found"))?;
            (
                tab.pty_id.clone(),
                tab.cwd.clone(),
                tab.agent_type.clone(),
                tab.role.is_some() || tab.roleless_tracked,
            )
        };
        reject_non_shell_restart(
            agent_type.as_deref(),
            agent_member,
            self.tab_chat_backing(tab_id).is_some(),
        )?;

        self.utility_command_finished(tab_id)?;

        if let Some(ref old_pty_id) = old_pty_id {
            self.pty_ownership.release_pty(old_pty_id);
            {
                let mut mux = self.mux.write();
                let tab = mux
                    .get_tab_mut(tab_id)
                    .ok_or_else(|| format!("Tab {tab_id} disappeared during restart"))?;
                tab.pty_id = None;
            }
            let reaper = {
                let mut manager = self.pty.lock();
                match manager.begin_kill_tree(old_pty_id) {
                    Ok(reaper) => Some(reaper),
                    Err(_) if manager.get_session(old_pty_id).is_none() => None,
                    Err(error) => return Err(error),
                }
            };
            if let Some(reaper) = reaper {
                reaper.wait_for_restart(Duration::from_secs(5))?;
            }
            self.screen_state.remove(old_pty_id);
            crate::service::stream_preview::remove(old_pty_id);
        }

        let resolved_cwd = cwd.or(current_cwd);
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.cwd = resolved_cwd.clone();
            tab.restored_from_snapshot = false;
            tab.shell_command_state = Default::default();
            tab.utility_command_pending = false;
        }
        self.ensure_tab_pty_for_send(tab_id)?;
        let new_pty_id = self.resolve_pty_id(tab_id)?;
        let command_submission = if let Some(command) = command {
            self.prepare_utility_command(tab_id, command);
            match self.submit_restarted_shell_command(tab_id, &new_pty_id, command) {
                Ok(submission) => Some(submission),
                Err(error) => {
                    if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
                        tab.utility_command_pending = false;
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        self.emit_state_changed(MuxEmitCause::AgentRestart);

        Ok(ShellRestartOutcome {
            tab_id: tab_id.to_string(),
            old_pty_id,
            new_pty_id,
            cwd: resolved_cwd,
            command_delivered: command_submission
                .as_ref()
                .is_some_and(ShellCommandSubmission::submitted),
            command_submission,
        })
    }
}

pub(crate) const SHELL_RESTART_AGENT_MEMBER: &str = "shell_restart_agent_member";

fn reject_non_shell_restart(
    agent_type: Option<&str>,
    agent_member: bool,
    chat_backend: bool,
) -> Result<(), String> {
    if chat_backend {
        return Err("tab restart only supports PTY shell tabs".into());
    }
    if agent_type.is_some() {
        return Err("tab restart only supports plain shells; use agent restart for agents".into());
    }
    // Exit teardown clears the provider but keeps the role; a hand-typed resume here launches without the granted access.
    if agent_member {
        return Err(format!(
            "{SHELL_RESTART_AGENT_MEMBER}: this agent Tab's provider exited; run `domios agent restart --tab <TAB>` to resume its verified session with the granted access"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
