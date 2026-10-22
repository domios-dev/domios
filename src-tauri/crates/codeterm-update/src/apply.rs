use super::channel::Channel;
use super::{InstallMethod, UpdatePhase, UpdateState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResult {
    NoUpdate,
    Restarting,
}

#[derive(Clone)]
pub struct ApplyOptions {
    pub channel: Channel,
    pub manifest_url: Option<String>,
    pub install_method: InstallMethod,
    /// Tab that initiated the update (the GA/caller running `update install`).
    /// Excluded from the shutdown sweep so it survives to hand off (Defect 3).
    pub initiating_tab_id: Option<String>,
}

pub fn begin_apply(update_state: &std::sync::Arc<parking_lot::Mutex<UpdateState>>) {
    let mut us = update_state.lock();
    us.set_phase(UpdatePhase::Checking);
    us.clear_error();
}

#[cfg(test)]
#[path = "apply/update_apply_phase_tests.rs"]
mod update_apply_phase_tests;
