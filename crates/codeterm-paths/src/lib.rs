//! Central data directory resolution.
//!
//! Dev builds (`cfg(debug_assertions)`) use `~/.codeterm-dev/`,
//! release builds use `~/.codeterm/`.
//! This keeps dev and prod fully isolated (config, sessions, crons, themes, sockets).

use std::path::PathBuf;

/// Sanitize a raw instance slug to `[a-z0-9-]+` (lowercase; other chars → `-`).
#[cfg(debug_assertions)]
fn sanitize_slug(raw: &str) -> String {
    raw.trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Suffix for the named dev instance selected via `CODETERM_INSTANCE`.
/// Debug only — release ignores the env var entirely (always prod).
pub fn instance_suffix() -> String {
    #[cfg(debug_assertions)]
    {
        match std::env::var("CODETERM_INSTANCE") {
            Ok(s) if !s.trim().is_empty() => format!("-{}", sanitize_slug(&s)),
            _ => String::new(),
        }
    }
    #[cfg(not(debug_assertions))]
    {
        String::new()
    }
}

/// True for debug/dev daemons — excluded from cross-machine resume targets.
pub fn is_dev_instance() -> bool {
    cfg!(debug_assertions)
}

/// Directory name for CodeTerm data (`~/.codeterm` or `~/.codeterm-dev[-<slug>]`).
pub fn dir_name() -> String {
    #[cfg(debug_assertions)]
    {
        format!(".codeterm-dev{}", instance_suffix())
    }
    #[cfg(not(debug_assertions))]
    {
        ".codeterm".to_string()
    }
}

/// Socket/pipe prefix for IPC (`codeterm` or `codeterm-dev[-<slug>]`).
pub fn socket_prefix() -> String {
    #[cfg(debug_assertions)]
    {
        format!("codeterm-dev{}", instance_suffix())
    }
    #[cfg(not(debug_assertions))]
    {
        "codeterm".to_string()
    }
}

/// Get the CodeTerm data directory (`~/.codeterm` or `~/.codeterm-dev`).
/// Creates the directory if it doesn't exist.
pub fn data_dir() -> PathBuf {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(dir_name());
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Get the config file path.
pub fn config_path() -> Result<PathBuf, String> {
    let dir = dirs::home_dir().ok_or_else(|| "Could not determine home directory".to_string())?;
    Ok(dir.join(dir_name()).join("config.toml"))
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
    let base = canonical_cwd(raw);
    if cfg!(windows) {
        base.to_lowercase()
    } else {
        base
    }
}

/// Canonicalize a cwd preserving case, with the Windows `\\?\` verbatim prefix
/// stripped. Use this (NOT `normalize_cwd`, which lowercases) when the result
/// feeds a path-encoding scheme where case matters — e.g. claude's
/// `~/.claude/projects/<encoded-cwd>` dir names keep the original case, so a
/// lowercased or verbatim-prefixed input encodes to a dir that doesn't exist.
pub fn canonical_cwd(raw: &str) -> String {
    let canonical = std::fs::canonicalize(raw)
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()));

    match canonical {
        Some(p) => strip_verbatim_prefix(&p),
        None => raw
            .trim_end_matches(|c: char| c == '/' || c == '\\')
            .to_string(),
    }
}

/// `std::fs::canonicalize` on Windows returns an extended-length path with a
/// `\\?\` (or `\\?\UNC\`) verbatim prefix. Agent CLIs encode the plain cwd, so
/// the prefix's extra leading slashes would corrupt any dir-name encoding.
/// Slash-separated `//?/` spellings are accepted too.
/// No-op on Unix paths.
pub fn strip_verbatim_prefix(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = p.strip_prefix("//?/UNC/") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = p.strip_prefix(r"\\?\") {
        rest.to_string()
    } else if let Some(rest) = p.strip_prefix("//?/") {
        rest.to_string()
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_verbatim_prefix_forms() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\C:\Users\User\.codeterm-dev\general"),
            r"C:\Users\User\.codeterm-dev\general"
        );
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\proj"),
            r"\\server\share\proj"
        );
        assert_eq!(
            strip_verbatim_prefix("//?/UNC/server/share/proj"),
            r"\\server\share\proj"
        );
        assert_eq!(
            strip_verbatim_prefix("//?/C:/Users/User/project"),
            "C:/Users/User/project"
        );
        assert_eq!(strip_verbatim_prefix("/home/user/p"), "/home/user/p");
        assert_eq!(strip_verbatim_prefix(r"C:\plain\path"), r"C:\plain\path");
    }

    #[test]
    fn canonical_cwd_preserves_case_and_strips_verbatim() {
        let tmp = std::env::temp_dir().to_string_lossy().to_string();
        let out = canonical_cwd(&tmp);
        assert!(!out.starts_with(r"\\?\"), "verbatim prefix leaked: {out}");

        assert_eq!(
            canonical_cwd(r"C:\Definitely\Missing\Path\"),
            r"C:\Definitely\Missing\Path"
        );
    }
}
