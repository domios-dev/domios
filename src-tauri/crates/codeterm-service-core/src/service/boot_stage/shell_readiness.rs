use super::{apply_boot_stage, BootStage, ServiceContext};
use crate::multiplexer::types::{Tab, TabBacking};

impl ServiceContext {
    pub(crate) fn observe_shell_ready(&self, pty_id: &str, generation: Option<u64>) {
        // Output batches are a hot path: only tabs still awaiting readiness may take the mux write lock.
        let tab_id = {
            let mux = self.mux.read();
            let Some(tab_id) = mux.tab_id_for_pty(pty_id) else {
                return;
            };
            if !mux
                .get_tab(&tab_id)
                .is_some_and(|tab| awaits_shell_ready(tab, pty_id))
            {
                return;
            }
            tab_id
        };
        let changed =
            self.record_boot_stage_transition(&tab_id, BootStage::ShellReady, None, |tab| {
                if !awaits_shell_ready(tab, pty_id) {
                    return false;
                }
                let Some(handle) = self.screen_state.existing(pty_id) else {
                    return false;
                };
                let mut screen = handle.lock();
                if generation.is_some_and(|generation| screen.pty_generation() != Some(generation))
                    || !screen.arm_shell_ready_signal().is_ready()
                {
                    return false;
                }
                apply_boot_stage(tab, BootStage::ShellReady, None);
                true
            });
        if changed {
            self.finish_boot_stage_transition(&tab_id, BootStage::ShellReady, None);
        }
    }
}

fn awaits_shell_ready(tab: &Tab, pty_id: &str) -> bool {
    tab.pty_id.as_deref() == Some(pty_id)
        && tab.agent_type.is_none()
        && !tab.agent_booting
        && matches!(tab.backing, TabBacking::Pty)
        && matches!(
            tab.boot_stage,
            None | Some(
                BootStage::PtyStarting
                    | BootStage::PtySpawned
                    | BootStage::ReaderReady
                    | BootStage::PtyReattached
            )
        )
}

#[cfg(test)]
mod tests;
