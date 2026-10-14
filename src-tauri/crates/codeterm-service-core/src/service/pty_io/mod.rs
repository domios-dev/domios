//! PTY I/O helpers: send text, batch send, resize, wait-for-pty.

pub(crate) const SHELL_BACKEND_COLS: u16 = 120;
pub(crate) const SEND_AGENT_BACKEND_COLS: u16 = 240;
#[cfg(windows)]
pub(crate) const PASTE_RENDER_WINDOW: std::time::Duration = std::time::Duration::from_millis(600);

mod batch;
mod control_key;
mod deliver;
mod inject;
mod lifecycle;
mod send;
pub(crate) mod submit_transaction;
pub(crate) mod tab_env;

pub(crate) use crate::service::ordered_actions::*;
#[cfg(test)]
pub(crate) use lifecycle::claim_restored_launch;
pub(crate) use lifecycle::claim_restored_pty_boot;
pub(crate) use send::build_spawn_dispatch;
pub(crate) use send::delivery_is_unknown;
pub use send::{await_bare_shell_prompt, await_shell_prompt};
pub use send::{
    await_bare_shell_prompt as poll_bare_shell_prompt, await_shell_prompt as poll_shell_prompt,
};
#[cfg(test)]
pub(crate) use tab_env::tab_identity_env;

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

pub use crate::ports::transport::SendBatchItem;

#[cfg(test)]
mod restore_replay_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
