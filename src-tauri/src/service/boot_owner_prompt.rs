//! A launch that still owes its startup acknowledgement is not ready while its
//! provider shows the owner a prompt: until the owner answers, the tab reports
//! `awaiting_owner_prompt` instead of `agent_ready`.

use super::boot_stage::BootStage;
use super::ServiceContext;
use std::collections::HashSet;
use std::sync::LazyLock;

static HELD: LazyLock<parking_lot::Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// The stage a surfaced owner prompt moves a tab to, if any.
pub(crate) fn held_stage(handshake_owed: bool, current: Option<BootStage>) -> Option<BootStage> {
    (handshake_owed && current == Some(BootStage::AgentReady))
        .then_some(BootStage::AwaitingOwnerPrompt)
}

/// The stage a cleared prompt restores, only for a tab this hold moved.
pub(crate) fn released_stage(held: bool, current: Option<BootStage>) -> Option<BootStage> {
    (held && current == Some(BootStage::AwaitingOwnerPrompt)).then_some(BootStage::AgentReady)
}

impl ServiceContext {
    fn current_boot_stage(&self, tab_id: &str) -> Option<BootStage> {
        self.mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.boot_stage)
    }

    pub(crate) fn hold_boot_for_owner_prompt(&self, tab_id: &str) {
        let owed = self.worker_ack.startup_handshake_owed(tab_id);
        let Some(stage) = held_stage(owed, self.current_boot_stage(tab_id)) else {
            return;
        };
        HELD.lock().insert(tab_id.to_string());
        log::info!(
            "[boot-stage] Tab {tab_id} shows an owner prompt before its startup acknowledgement"
        );
        self.emit_boot_stage(tab_id, stage, None);
    }

    pub(crate) fn release_boot_owner_prompt(&self, tab_id: &str) {
        let held = HELD.lock().remove(tab_id);
        if let Some(stage) = released_stage(held, self.current_boot_stage(tab_id)) {
            self.emit_boot_stage(tab_id, stage, None);
        }
    }
}

#[cfg(test)]
#[path = "boot_owner_prompt/tests.rs"]
mod tests;
