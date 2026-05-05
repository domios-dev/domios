//! Thin Tauri command shims for hook installation. The actual logic
//! lives inside each provider plugin (`plugin/agent_<id>/hooks.rs`) so
//! the hook lifecycle stays consolidated with the rest of that
//! provider's code (detection, sessions, TUI control). This file is
//! pure adapter — every fn body delegates to the matching plugin
//! module without holding state of its own.

use crate::plugin::{agent_claude, agent_codex};

pub use agent_claude::hooks::{ClaudeHooksStatus, ClaudeTokenHooksStatus};
pub use agent_codex::hooks::CodexTokenHooksStatus;

#[tauri::command]
pub fn check_claude_hooks() -> Result<ClaudeHooksStatus, String> {
    agent_claude::hooks::check_v2_hooks()
}

#[tauri::command]
pub fn install_claude_hooks(api_port: Option<u16>) -> Result<(), String> {
    agent_claude::hooks::install_v2_hooks(api_port)
}

#[tauri::command]
pub fn uninstall_claude_hooks() -> Result<(), String> {
    agent_claude::hooks::uninstall_v2_hooks()
}

#[tauri::command]
pub fn check_claude_token_hooks() -> Result<ClaudeTokenHooksStatus, String> {
    agent_claude::hooks::check_token_hooks()
}

#[tauri::command]
pub fn install_claude_token_hooks() -> Result<(), String> {
    agent_claude::hooks::install_token_hooks()
}

#[tauri::command]
pub fn uninstall_claude_token_hooks() -> Result<(), String> {
    agent_claude::hooks::uninstall_token_hooks()
}

#[tauri::command]
pub fn check_codex_token_hooks() -> Result<CodexTokenHooksStatus, String> {
    agent_codex::hooks::check_token_hooks()
}

#[tauri::command]
pub fn install_codex_token_hooks() -> Result<(), String> {
    agent_codex::hooks::install_token_hooks()
}

#[tauri::command]
pub fn uninstall_codex_token_hooks() -> Result<(), String> {
    agent_codex::hooks::uninstall_token_hooks()
}
