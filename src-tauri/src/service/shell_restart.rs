use std::time::Duration;

use serde::Serialize;

use super::ServiceContext;

#[derive(Debug, Serialize)]
pub struct ShellRestartOutcome {
    pub tab_id: String,
    pub old_pty_id: Option<String>,
    pub new_pty_id: String,
    pub cwd: Option<String>,
}

impl ServiceContext {
    pub fn restart_shell_tab(
        &self,
        tab_id: &str,
        cwd: Option<String>,
    ) -> Result<ShellRestartOutcome, String> {
        let (old_pty_id, current_cwd, agent_type) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {tab_id} not found"))?;
            (tab.pty_id.clone(), tab.cwd.clone(), tab.agent_type.clone())
        };
        reject_non_shell_restart(
            agent_type.as_deref(),
            self.tab_chat_backing(tab_id).is_some(),
        )?;

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
                match manager.begin_kill(old_pty_id) {
                    Ok(reaper) => Some(reaper),
                    Err(_) if manager.get_session(old_pty_id).is_none() => None,
                    Err(error) => return Err(error),
                }
            };
            if let Some(reaper) = reaper {
                reaper.wait_bounded(Duration::from_secs(5))?;
            }
            self.screen_state.lock().remove(old_pty_id);
            crate::service::stream_preview::remove(old_pty_id);
        }

        let resolved_cwd = cwd.or(current_cwd);
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.cwd = resolved_cwd.clone();
            tab.restored_from_snapshot = false;
        }
        self.ensure_tab_pty_for_send(tab_id)?;
        let new_pty_id = self.resolve_pty_id(tab_id)?;
        self.emit_state_changed();

        Ok(ShellRestartOutcome {
            tab_id: tab_id.to_string(),
            old_pty_id,
            new_pty_id,
            cwd: resolved_cwd,
        })
    }
}

fn reject_non_shell_restart(agent_type: Option<&str>, chat_backend: bool) -> Result<(), String> {
    if chat_backend {
        return Err("tab restart only supports PTY shell tabs".into());
    }
    if agent_type.is_some() {
        return Err("tab restart only supports plain shells; use agent restart for agents".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::reject_non_shell_restart;

    #[test]
    fn shell_restart_rejects_agent_and_chat_backend_tabs() {
        assert!(reject_non_shell_restart(Some("codex"), false).is_err());
        assert!(reject_non_shell_restart(None, true).is_err());
        assert!(reject_non_shell_restart(None, false).is_ok());
    }
}
