//! PTY I/O helpers: send text, batch send, resize, wait-for-pty.
use serde::Deserialize;

pub(crate) const SHELL_BACKEND_COLS: u16 = 120;
pub(crate) const SEND_AGENT_BACKEND_COLS: u16 = 240;

mod inject;
mod lifecycle;
mod send;

pub(crate) use send::build_spawn_dispatch;
pub use send::poll_shell_prompt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoredAgentMaterialization {
    Resume,
    Fresh,
}

pub(crate) fn restored_agent_materialization(
    restored_from_snapshot: bool,
    protected: bool,
    agent_type: Option<&str>,
    session_id: Option<&str>,
    session_valid: bool,
) -> Option<RestoredAgentMaterialization> {
    if !restored_from_snapshot || protected || agent_type.is_none() {
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
