//! General Agent — always-on system agent lifecycle management.
//!
//! Layout:
//!   - [`instructions`] — AGENT.md / CLAUDE.md / AGENTS.md / bootstrap.md generation
//!     plus the `sync_identity_to_mem` helper.
//!   - [`workspace`] — General workspace + protected pane setup.
//!   - [`lifecycle`] — start / restart of the GA PTY + launch flow.
//!   - [`watchdog`] — periodic health-check + auto-restart.
//!
//! Public surface is re-exported below so external callers continue to use
//! `service::general_agent::ensure_directories`, `start_general_agent`,
//! `GENERAL_ALIAS`, etc., unchanged.

pub mod blocker;
mod instructions;
mod lifecycle;
mod watchdog;
mod workspace;

use std::path::PathBuf;

pub use instructions::{generate_instructions, sync_identity_to_mem};
pub use lifecycle::{restart_general_agent, start_general_agent};
pub use watchdog::start_watchdog;
pub use workspace::ensure_general_workspace;

pub const GENERAL_ALIAS: &str = "general";
/// Second alias for the GA pane — `codeterm pane ... main` resolves to it too.
pub const MAIN_ALIAS: &str = "main";
pub const GENERAL_WORKSPACE_NAME: &str = "General";
pub const GENERAL_PANE_TITLE: &str = "Main";

/// Get the General Agent workspace directory.
/// `~/.codeterm/general/` (or `~/.codeterm-dev/general/` for dev builds).
pub fn general_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("general")
}

/// Get the secrets directory.
pub fn secrets_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("secrets")
}

/// Get the integrations config directory.
pub fn integrations_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::DIR_NAME).join("integrations")
}

/// Ensure all General Agent directories exist.
pub fn ensure_directories() {
    let dirs = [
        general_dir(),
        secrets_dir(),
        integrations_dir(),
        general_dir().join("state"),
    ];
    for dir in &dirs {
        if !dir.exists() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::error!(
                    "[general-agent] Failed to create directory {:?}: {}",
                    dir,
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_general_dir_under_home() {
        let dir = general_dir();
        let home = dirs::home_dir().unwrap();
        assert!(dir.starts_with(home));
        assert!(dir.ends_with("general"));
    }

    #[test]
    fn test_secrets_dir_under_home() {
        let dir = secrets_dir();
        let home = dirs::home_dir().unwrap();
        assert!(dir.starts_with(home));
        assert!(dir.ends_with("secrets"));
    }
}
