use crate::ports::host::UiEvents;
pub use codeterm_keepawake::*;
use std::sync::Arc;

struct UiStatusEmitter(Arc<dyn UiEvents>);

impl Clone for UiStatusEmitter {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl StatusEmitter for UiStatusEmitter {
    fn emit_status(&self, status: &KeepAwakeStatus) {
        if let Ok(payload) = serde_json::to_value(status) {
            self.0.emit("keepawake-changed", payload);
        }
    }
}

pub fn start(
    state: &KeepAwakeState,
    ui: &Arc<dyn UiEvents>,
    duration_secs: Option<u64>,
    display: Option<bool>,
    gnome_policy: Option<bool>,
    system_block: Option<bool>,
) -> Result<KeepAwakeStatus, String> {
    codeterm_keepawake::start(
        state,
        &UiStatusEmitter(ui.clone()),
        duration_secs,
        display,
        gnome_policy,
        system_block,
    )
}

pub fn stop(state: &KeepAwakeState, ui: &Arc<dyn UiEvents>) -> Result<(), String> {
    codeterm_keepawake::stop(state, &UiStatusEmitter(ui.clone()))
}

pub fn resume_from_disk(state: &KeepAwakeState, ui: &Arc<dyn UiEvents>) {
    codeterm_keepawake::resume_from_disk(state, &UiStatusEmitter(ui.clone()));
}

pub fn start_supervisor(state: KeepAwakeState, ui: Arc<dyn UiEvents>) {
    codeterm_keepawake::start_supervisor(state, UiStatusEmitter(ui));
}

#[cfg(test)]
mod tests;
