//! PTY I/O helpers: send text, batch send, resize, wait-for-pty.
use serde::Deserialize;

pub(crate) const SHELL_BACKEND_COLS: u16 = 120;
pub(crate) const SEND_AGENT_BACKEND_COLS: u16 = 240;

mod inject;
mod lifecycle;
mod send;
mod wake;

#[cfg(test)]
pub(crate) use lifecycle::{active_pane_lane_base, pane_task_env};
pub(crate) use send::build_spawn_dispatch;
pub(crate) use send::provider_or_generic_tui;
pub use send::{poll_bare_shell_prompt, poll_shell_prompt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoredAgentMaterialization {
    Resume,
    Fresh,
}

pub(crate) fn restored_agent_materialization(
    restored_from_snapshot: bool,
    agent_booting: bool,
    protected: bool,
    agent_type: Option<&str>,
    session_id: Option<&str>,
    session_valid: bool,
) -> Option<RestoredAgentMaterialization> {
    if !restored_from_snapshot || agent_booting || protected || agent_type.is_none() {
        return None;
    }
    match session_id {
        Some(_) if session_valid => Some(RestoredAgentMaterialization::Resume),
        _ => Some(RestoredAgentMaterialization::Fresh),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendBatchItem {
    pub text: String,
    #[serde(default)]
    pub enter: bool,
    pub delay_ms: Option<u64>,
}

#[cfg(test)]
mod tests;
