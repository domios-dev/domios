// Shared inline types used as command return types live here so external
// callers can reference them as `commands::DirCompletion`, etc.

#[derive(serde::Serialize)]
pub struct DirCompletion {
    pub name: String,
    pub score: i32,
}

#[derive(serde::Serialize)]
pub struct RuleStatus {
    pub installed: bool,
    pub update_available: bool,
    pub revision: u32,
    pub installed_revision: u32,
}

// `ClaudeHooksStatus`, `ClaudeTokenHooksStatus`, `CodexTokenHooksStatus`
// now live in their owning plugin (`plugin/agent_<id>/hooks.rs`). The
// thin Tauri shims in `commands/hooks.rs` re-export them so external
// callers see the same wire shape without core knowing per-provider
// details.

mod agent;
mod clipboard;
mod completions;
mod cron;
mod hooks;
mod pane;
mod pty;
mod skills;
mod system;
mod system_info;
mod update;

#[cfg(test)]
mod tests;

pub use agent::*;
pub use clipboard::*;
pub use completions::*;
pub use cron::*;
pub use hooks::*;
pub use pane::*;
pub use pty::*;
pub use skills::*;
pub use system::*;
pub use system_info::*;
pub use update::*;
