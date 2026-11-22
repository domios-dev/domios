use codeterm_types::pty::TimelineState;
use codeterm_update::holders::{BusyTab, HolderProcess, HolderTab};

use crate::multiplexer::types::{ShellActivity, Tab};

mod reservation;
mod shell_restore;
pub use reservation::PtyInputRetirement;

impl super::ServiceContext {
    pub fn describe_update_holders(
        &self,
        processes: Vec<HolderProcess>,
    ) -> Result<Vec<HolderTab>, String> {
        let mut tabs = Vec::new();
        for process in processes {
            if let Some(tab_id) = self.update_holder_tab(&process)? {
                let (pty_id, session) = {
                    let mux = self.mux.read();
                    let tab = mux
                        .get_tab(&tab_id)
                        .ok_or("update holder tab disappeared")?;
                    let pty_id = tab.pty_id.clone().ok_or("update holder PTY disappeared")?;
                    let session = self
                        .pty
                        .lock()
                        .get_session(&pty_id)
                        .ok_or("update holder PTY disappeared")?;
                    (pty_id, session)
                };
                let input_revision = session.write_queue.input_revision();
                let command = self.running_utility_command(&tab_id);
                let unsettled = self.chat_send_ledger.has_unsettled_update_input(&tab_id);
                let mux = self.mux.read();
                let Some(tab) = mux.get_tab(&tab_id) else {
                    continue;
                };
                if tab.pty_id.as_deref() != Some(pty_id.as_str()) {
                    return Err(format!(
                        "Update blocked: {} changed its PTY; retry the update",
                        tab.title
                    ));
                }
                let workspace = mux
                    .find_workspace_for_tab(&tab_id)
                    .and_then(|id| mux.get_workspace(&id))
                    .map(|ws| ws.name.clone())
                    .unwrap_or_else(|| "Unknown workspace".into());
                let busy = self.update_block_reason(tab, true).is_some() || unsettled;
                let holder = HolderTab {
                    process,
                    tab_id: tab_id.clone(),
                    title: tab.title.clone(),
                    workspace,
                    command: command
                        .or_else(|| {
                            tab.agent_state
                                .as_ref()
                                .and_then(|state| state.status_line.clone())
                        })
                        .or_else(|| tab.description.clone())
                        .or_else(|| tab.agent_type.clone())
                        .unwrap_or_else(|| "shell (activity unknown)".into()),
                    busy,
                    pty_id,
                    pty_generation: session.generation,
                    input_revision,
                };
                tabs.push(holder);
            }
        }
        Ok(tabs)
    }

    fn require_update_ready(
        &self,
        initiating_tab: Option<&str>,
        require_idle: bool,
    ) -> Result<(), String> {
        use crate::multiplexer::types::{TabBacking, TabType};

        let mux = self.mux.read();
        let candidates: Vec<_> = mux
            .all_tabs()
            .filter(|tab| Some(tab.id.as_str()) != initiating_tab)
            .filter(|tab| match tab.backing {
                TabBacking::Pty => tab.pty_id.is_some() || tab.agent_booting,
                TabBacking::ChatBackend { .. } => tab.agent_type.is_some() && !tab.frozen,
                TabBacking::Surface(_) => false,
            })
            .filter(|tab| tab.tab_type == TabType::Terminal)
            .filter_map(|tab| {
                let reason = self.update_block_reason(tab, require_idle);
                if reason.is_none() && !require_idle {
                    return None;
                }
                let row = BusyTab {
                    tab_id: tab.id.clone(),
                    title: tab.title.clone(),
                    workspace: mux
                        .find_workspace_for_tab(&tab.id)
                        .and_then(|id| mux.get_workspace(&id))
                        .map(|workspace| workspace.name.clone())
                        .unwrap_or_else(|| "Unknown workspace".into()),
                    command: tab
                        .agent_state
                        .as_ref()
                        .and_then(|state| state.status_line.clone())
                        .unwrap_or_default(),
                };
                Some((reason, row))
            })
            .collect();
        drop(mux);
        let busy = candidates.into_iter().filter_map(|(reason, mut tab)| {
            let reason = reason.or_else(|| {
                (require_idle
                    && self
                        .chat_send_ledger
                        .has_unsettled_update_input(&tab.tab_id))
                .then_some("Input delivery has not settled")
            })?;
            tab.command = if tab.command.is_empty() {
                reason.into()
            } else {
                format!("{reason} ({})", tab.command)
            };
            Some(tab)
        });
        codeterm_update::holders::refusal(busy).map_or(Ok(()), Err)
    }

    fn update_block_reason(&self, tab: &Tab, require_idle: bool) -> Option<&'static str> {
        if require_idle && (tab.utility_command_pending || tab.pending_delivery.is_some()) {
            return Some("Input delivery has not settled");
        }
        if !tab.is_utility_shell() || tab.agent_booting {
            return (require_idle
                && tab.agent_state.as_ref().map(|state| state.state) != Some(TimelineState::Idle))
            .then_some("An idle agent is required by the updater settings");
        }
        match tab.shell_command_state.activity {
            ShellActivity::Running => self
                .shutdown_shell_command(tab)
                .is_none()
                .then_some("The running shell command could not be saved for rerun"),
            ShellActivity::Unknown if require_idle => Some("Shell activity could not be verified"),
            _ => None,
        }
    }

    #[cfg(not(windows))]
    fn update_holder_tab(&self, _: &HolderProcess) -> Result<Option<String>, String> {
        Ok(None)
    }

    #[cfg(windows)]
    fn update_holder_tab(&self, process: &HolderProcess) -> Result<Option<String>, String> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::{Foundation::FILETIME, System::Threading::*};
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process.pid) };
        if raw.is_null() {
            return Ok(None);
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetProcessTimes(raw, &mut created, &mut exit, &mut kernel, &mut user) } == 0 {
            return Err("cannot verify update holder creation time".into());
        }
        if ((created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64) != process.created
        {
            return Ok(None);
        }
        let mux = self.mux.read();
        let pty_id = self.pty.lock().owning_pty(handle.as_raw_handle())?;
        Ok(pty_id.and_then(|id| {
            mux.all_tabs()
                .find(|tab| tab.pty_id.as_ref() == Some(&id))
                .map(|tab| tab.id.clone())
        }))
    }
}

#[cfg(test)]
mod tests;
