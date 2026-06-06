//! Central data directory resolution.
//!
//! Dev builds (`cfg(debug_assertions)`) use `~/.codeterm-dev/`,
//! release builds use `~/.codeterm/`.
//! This keeps dev and prod fully isolated (config, sessions, crons, themes, sockets).

use std::path::PathBuf;

/// Directory name for CodeTerm data.
#[cfg(debug_assertions)]
pub const DIR_NAME: &str = ".codeterm-dev";

#[cfg(not(debug_assertions))]
pub const DIR_NAME: &str = ".codeterm";

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
    let dir = dirs::home_dir().ok_or_else(|| "Could not determine home directory".to_string())?;
    Ok(dir.join(DIR_NAME).join("config.toml"))
}

/// Normalize a workspace `cwd` for comparison purposes. Two inputs that refer
/// to the same folder should produce equal outputs regardless of trailing
/// slashes, symlinks, or Windows case/separator differences.
///
/// - Runs `std::fs::canonicalize` when the path exists (resolves symlinks,
///   converts to absolute, normalizes separators).
/// - Falls back to manual trimming of trailing separators if canonicalize
///   fails (path doesn't exist, permission denied, etc.) so dedup still
///   works best-effort.
/// - Lowercases the whole string on Windows — NTFS is case-insensitive by
///   default and `C:\Users\X` == `c:\users\x`.
pub fn normalize_cwd(raw: &str) -> String {
    let canonical = std::fs::canonicalize(raw)
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()));

    let base = canonical.unwrap_or_else(|| {
        raw.trim_end_matches(|c: char| c == '/' || c == '\\')
            .to_string()
    });

    if cfg!(windows) {
        // Strip the Windows verbatim prefix that canonicalize adds on Windows
        // (`\\?\C:\foo`) so user-supplied and canonicalized paths still match.
        let stripped = base.strip_prefix(r"\\?\").unwrap_or(&base);
        stripped.to_lowercase()
    } else {
        base
    }
}
