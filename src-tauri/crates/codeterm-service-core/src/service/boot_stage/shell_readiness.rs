use super::{apply_boot_stage, BootStage, ServiceContext};
use crate::multiplexer::types::TabBacking;

impl ServiceContext {
    pub(crate) fn observe_shell_ready(&self, pty_id: &str, generation: Option<u64>) {
        let Some(tab_id) = self.mux.read().tab_id_for_pty(pty_id) else {
            return;
        };
        let changed =
            self.record_boot_stage_transition(&tab_id, BootStage::ShellReady, None, |tab| {
                if tab.pty_id.as_deref() != Some(pty_id)
                    || tab.agent_type.is_some()
                    || tab.agent_booting
                    || !matches!(tab.backing, TabBacking::Pty)
                    || !matches!(
                        tab.boot_stage,
                        None | Some(
                            BootStage::PtyStarting
                                | BootStage::PtySpawned
                                | BootStage::ReaderReady
                                | BootStage::PtyReattached
                        )
                    )
                {
                    return false;
                }
                let mut screen = self.screen_state.lock();
                if generation
                    .is_some_and(|generation| screen.pty_generation(pty_id) != Some(generation))
                    || !screen.arm_shell_ready_signal(pty_id).is_ready()
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

#[cfg(test)]
mod tests;
