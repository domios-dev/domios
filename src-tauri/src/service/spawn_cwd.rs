//! An agent runs where its tab says it runs. The PTY layer silently falls back
//! to the home directory for a cwd that is not a directory, so one is refused here.

use std::path::Path;

#[cfg(test)]
#[path = "spawn_cwd/tests.rs"]
mod tests;

/// A spawn's requested cwd must be an absolute, existing directory on this host.
/// Relative paths are resolved by the caller, which alone knows its own directory.
pub(crate) fn validate_spawn_cwd(cwd: Option<&str>) -> Result<(), String> {
    let Some(cwd) = cwd.filter(|cwd| !cwd.trim().is_empty()) else {
        return Ok(());
    };
    match cwd_problem(cwd) {
        Some(problem) => Err(format!("invalid --cwd: {problem}")),
        None => Ok(()),
    }
}

/// Why this host cannot start a process in `cwd`, or `None` when it can.
pub(crate) fn cwd_problem(cwd: &str) -> Option<String> {
    let path = Path::new(cwd);
    if !path.is_absolute() {
        return Some(format!(
            "'{cwd}' is not an absolute path on this host{}",
            foreign_path_hint(cwd)
        ));
    }
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => None,
        Ok(_) => Some(format!("'{cwd}' is not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(format!("'{cwd}' does not exist"))
        }
        Err(error) => Some(format!("'{cwd}' cannot be inspected: {error}")),
    }
}

fn foreign_path_hint(cwd: &str) -> &'static str {
    let bytes = cwd.as_bytes();
    let drive_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if drive_path {
        "; this is a Windows path on a non-Windows host (Git Bash rewrites POSIX arguments unless MSYS_NO_PATHCONV=1 is set)"
    } else {
        "; pass an absolute directory"
    }
}
