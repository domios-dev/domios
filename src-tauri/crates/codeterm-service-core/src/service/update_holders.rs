use codeterm_update::holders::{BusyTab, HolderProcess, HolderTab};

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
                let busy = holder_is_busy(
                    tab.agent_type.is_some(),
                    tab.agent_state.as_ref().map(|state| state.state),
                    tab.shell_command_state.activity,
                ) || tab.utility_command_pending
                    || tab.pending_delivery.is_some()
                    || unsettled;
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

    fn require_idle_update(&self, initiating_tab: Option<&str>) -> Result<(), String> {
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
            .map(|tab| {
                let busy = holder_is_busy(
                    tab.agent_type.is_some(),
                    tab.agent_state.as_ref().map(|state| state.state),
                    tab.shell_command_state.activity,
                ) || tab.utility_command_pending
                    || tab.pending_delivery.is_some();
                let row = BusyTab {
                    tab_id: tab.id.clone(),
                    title: tab.title.clone(),
                    workspace: mux
                        .find_workspace_for_tab(&tab.id)
                        .and_then(|id| mux.get_workspace(&id))
                        .map(|workspace| workspace.name.clone())
                        .unwrap_or_else(|| "Unknown workspace".into()),
                    command: self
                        .running_utility_command(&tab.id)
                        .or_else(|| {
                            tab.agent_state
                                .as_ref()
                                .and_then(|state| state.status_line.clone())
                        })
                        .unwrap_or_else(|| "activity unavailable".into()),
                };
                (busy, row)
            })
            .collect();
        drop(mux);
        let busy = candidates.into_iter().filter_map(|(busy, tab)| {
            (busy
                || self
                    .chat_send_ledger
                    .has_unsettled_update_input(&tab.tab_id))
            .then_some(tab)
        });
        codeterm_update::holders::refusal(busy).map_or(Ok(()), Err)
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

fn holder_is_busy(
    agent: bool,
    state: Option<codeterm_types::pty::TimelineState>,
    shell: crate::multiplexer::types::ShellActivity,
) -> bool {
    if agent {
        state != Some(codeterm_types::pty::TimelineState::Idle)
    } else {
        shell != crate::multiplexer::types::ShellActivity::Idle
    }
}

#[cfg(test)]
mod tests;
