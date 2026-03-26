//! Central data directory resolution.
//!
//! Dev builds (`cfg(debug_assertions)`) use `~/.codeterm-dev/`,
//! release builds use `~/.codeterm/`.
//! This keeps dev and prod fully isolated (config, sessions, crons, themes, sockets).

use std::path::PathBuf;

/// Directory name for CodeTerm data.
#[cfg(debug_assertions)]
const DIR_NAME: &str = ".codeterm-dev";

#[cfg(not(debug_assertions))]
const DIR_NAME: &str = ".codeterm";

/// Socket/pipe prefix for IPC.
#[cfg(debug_assertions)]
pub const SOCKET_PREFIX: &str = "codeterm-dev";

#[cfg(not(debug_assertions))]
pub const SOCKET_PREFIX: &str = "codeterm";

/// Get the CodeTerm data directory (`~/.codeterm` or `~/.codeterm-dev`).
/// Creates the directory if it doesn't exist.
pub fn data_dir() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(DIR_NAME);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Get the config file path.
pub fn config_path() -> Result<PathBuf, String> {
    let dir = dirs::home_dir()
        .ok_or_else(|| "Could not determine home directory".to_string())?;
    Ok(dir.join(DIR_NAME).join("config.toml"))
}
