//! PTY I/O helpers: send text, batch send, resize, wait-for-pty.
use serde::Deserialize;

pub(crate) const SHELL_BACKEND_COLS: u16 = 120;
pub(crate) const SEND_AGENT_BACKEND_COLS: u16 = 240;
pub(crate) const PASTE_RENDER_WINDOW: std::time::Duration = std::time::Duration::from_millis(600);

mod actions;
mod batch;
mod inject;
mod lifecycle;
mod send;
pub(crate) mod submit_transaction;

pub(crate) use actions::{await_action_deadline, execute_ordered_actions, OrderedPtyAction};
pub(crate) use lifecycle::claim_restored_pty_boot;
#[cfg(test)]
pub(crate) use lifecycle::tab_identity_env;
pub(crate) use send::build_spawn_dispatch;
pub(crate) use send::provider_or_generic_tui;
pub(crate) use send::sanitize_agent_prompt_input;
pub use send::{await_bare_shell_prompt, await_shell_prompt};
pub use send::{
    await_bare_shell_prompt as poll_bare_shell_prompt, await_shell_prompt as poll_shell_prompt,
};

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
mod restore_replay_tests;
#[cfg(test)]
mod tests;
