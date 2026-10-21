//! Central data directory resolution.
//!
//! Dev builds (`cfg(debug_assertions)`) use `~/.domios-dev/`,
//! release builds use `~/.domios/`.
//! This keeps dev and prod fully isolated (config, sessions, crons, themes, sockets).

use std::path::PathBuf;

pub mod instance_identity;
mod port_policy;
pub use port_policy::{runtime_api_port, validate_listener_port, PRODUCTION_PORT};

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

fn identity_names_for(dev: bool, suffix: &str) -> (String, String) {
    if dev {
        (
            format!(".domios-dev{suffix}"),
            format!("domios-dev{suffix}"),
        )
    } else {
        (".domios".to_string(), "domios".to_string())
    }
}

/// Directory name for Domios data (`~/.domios` or `~/.domios-dev[-<slug>]`).
pub fn dir_name() -> String {
    identity_names_for(is_dev_instance(), &instance_suffix()).0
}

/// Socket/pipe prefix for IPC (`domios` or `domios-dev[-<slug>]`).
pub fn socket_prefix() -> String {
    identity_names_for(is_dev_instance(), &instance_suffix()).1
}

/// Bare namespaced socket name for the current process (`GenericNamespaced` adds `/tmp/` on Unix).
pub fn socket_path() -> String {
    #[cfg(debug_assertions)]
    {
        format!("{}.sock", socket_prefix())
    }
    #[cfg(not(debug_assertions))]
    {
        let pid = std::process::id();
        format!("{}-{}.sock", socket_prefix(), pid)
    }
}

/// Full socket display path: `/tmp/<name>` on Unix, `\.\pipe\<name>` on Windows.
pub fn socket_display_path() -> String {
    let bare = socket_path();

    #[cfg(target_os = "windows")]
    {
        format!("\\\\.\\pipe\\{}", bare)
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!("/tmp/{}", bare)
    }
}

/// Resolve the Domios data directory without creating it.
pub fn data_dir_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CODETERM_DATA_DIR").map(PathBuf::from) {
        if path.is_absolute() {
            return path;
        }
    }
    default_data_dir().unwrap_or_else(|| PathBuf::from(".").join(dir_name()))
}

#[cfg(not(feature = "isolated-data-root"))]
fn default_data_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(dir_name()))
}

/// Test builds swap the per-user root for one process-wide temp root so no test reaches it.
#[cfg(feature = "isolated-data-root")]
fn default_data_dir() -> Option<PathBuf> {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let root = ROOT.get_or_init(|| {
        tempfile::Builder::new()
            .prefix("domios-test-root-")
            .tempdir()
            .expect("create isolated test data root")
    });
    Some(root.path().join(dir_name()))
}

/// Get the Domios data directory, honoring an absolute explicit override.
/// Creates the selected directory if it doesn't exist.
pub fn data_dir() -> PathBuf {
    let dir = data_dir_path();
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Get the config file path, honoring explicit config and data directory overrides.
pub fn config_path() -> Result<PathBuf, String> {
    if let Some(config_dir) = std::env::var_os("CODETERM_CONFIG_DIR").map(PathBuf::from) {
        if !config_dir.is_absolute() {
            return Err("CODETERM_CONFIG_DIR must be absolute".to_string());
        }
        return Ok(config_dir.join("config.toml"));
    }
    if let Some(data_dir) = std::env::var_os("CODETERM_DATA_DIR").map(PathBuf::from) {
        if data_dir.is_absolute() {
            return Ok(data_dir.join("config.toml"));
        }
    }
    let dir = default_data_dir().ok_or_else(|| "Could not determine home directory".to_string())?;
    Ok(dir.join("config.toml"))
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

/// Compare cwd spellings without filesystem access. This is safe inside state
/// locks and treats Windows verbatim prefixes as presentation only.
pub fn paths_equal(a: &str, b: &str) -> bool {
    let normalize = |path: &str| {
        let path = strip_verbatim_prefix(path).replace('\\', "/");
        path.trim_end_matches('/').to_string()
    };
    let a = normalize(a);
    let b = normalize(b);
    #[cfg(windows)]
    {
        a.eq_ignore_ascii_case(&b)
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// Canonicalize a cwd preserving case, with the Windows `\\?\` verbatim prefix
/// stripped. Use this (NOT `normalize_cwd`, which lowercases) when the result
/// feeds a path-encoding scheme where case matters — e.g. claude's
/// `~/.claude/projects/<encoded-cwd>` dir names keep the original case, so a
/// lowercased or verbatim-prefixed input encodes to a dir that doesn't exist.
pub fn canonical_cwd(raw: &str) -> String {
    let raw = strip_verbatim_prefix(raw);
    let canonical = std::fs::canonicalize(&raw)
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
/// Slash-separated `//?/` spellings normalize to Windows backslashes on every host.
/// No-op on Unix paths.
pub fn strip_verbatim_prefix(p: &str) -> String {
    let rest = p.strip_prefix(r"\\?\").or_else(|| p.strip_prefix("//?/"));
    let Some(rest) = rest else {
        return p.to_string();
    };
    let normalized = rest.replace('/', "\\");
    if normalized
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("UNC\\"))
    {
        let unc = &normalized[4..];
        let mut parts = unc.split('\\');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        if !server.is_empty() && !share.is_empty() {
            return format!(r"\\{unc}");
        }
    }
    let bytes = normalized.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        return normalized;
    }
    p.to_string()
}

#[cfg(test)]
mod tests;
