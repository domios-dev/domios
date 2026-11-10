use std::cell::Cell;

use crate::multiplexer::types::{ShellActivity, ShellCommandState};
use crate::service::delivery_error::DeliveryError;
use crate::service::ServiceContext;

pub(super) fn busy_refused(pty_id: &str, detail: &str) -> String {
    DeliveryError::TabBusyRefused {
        tab: pty_id.into(),
        detail: detail.into(),
    }
    .ledger_message()
}

pub(super) fn shell_command_running(svc: &ServiceContext, pty_id: &str) -> bool {
    svc.ext::<crate::ShellCapabilityState>()
        .lock()
        .command_state(pty_id)
        .activity
        == ShellActivity::Running
}

pub(crate) struct UtilityCommandAdmission<'a> {
    svc: &'a ServiceContext,
    tab_id: String,
    pty_id: String,
    previous: ShellCommandState,
    written: Cell<bool>,
}

impl<'a> UtilityCommandAdmission<'a> {
    pub(crate) fn begin(svc: &'a ServiceContext, pty_id: &str) -> Result<Self, String> {
        if shell_command_running(svc, pty_id) {
            return Err(busy_refused(pty_id, "a shell command is running"));
        }
        let mut mux = svc.mux.write();
        let tab_id = mux
            .tab_id_for_pty(pty_id)
            .ok_or_else(|| format!("pty_unavailable: no tab owns PTY {pty_id}"))?;
        let tab = mux
            .get_tab_mut(&tab_id)
            .ok_or_else(|| format!("pty_unavailable: tab {tab_id} disappeared"))?;
        if tab.agent_type.is_some()
            || tab.agent_booting
            || tab.utility_command_pending
            || tab.shell_command_state.activity == ShellActivity::Running
        {
            return Err(busy_refused(pty_id, "shell input is already owned"));
        }
        let previous = tab.shell_command_state;
        tab.utility_command_pending = true;
        tab.shell_command_state = Default::default();
        Ok(Self {
            svc,
            tab_id,
            pty_id: pty_id.into(),
            previous,
            written: Cell::new(false),
        })
    }

    pub(crate) fn mark_written(&self) {
        self.written.set(true);
    }
}

impl Drop for UtilityCommandAdmission<'_> {
    fn drop(&mut self) {
        if self.written.get() {
            return;
        }
        let mut mux = self.svc.mux.write();
        if let Some(tab) = mux.get_tab_mut(&self.tab_id).filter(|tab| {
            tab.pty_id.as_deref() == Some(&self.pty_id) && tab.utility_command_pending
        }) {
            tab.utility_command_pending = false;
            tab.shell_command_state = self.previous;
        }
    }
}

#[cfg(test)]
mod tests;
