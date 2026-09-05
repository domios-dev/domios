//! Shared helpers for assembling readable crash reports.

use std::fmt::Write;
use std::path::PathBuf;

pub fn crashes_dir() -> PathBuf {
    let dir = crate::paths::data_dir().join("crashes");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn timestamp_slug() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

pub fn header(kind: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "CodeTerm crash report");
    let _ = writeln!(s, "=====================");
    let _ = writeln!(s, "Type: {}", kind);
    let _ = writeln!(s, "Timestamp: {}", chrono::Utc::now().to_rfc3339());
    let _ = writeln!(
        s,
        "Version: {} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("BUILD_HASH")
    );
    let _ = writeln!(s, "Image: {}", super::image_identity::image_path());
    let _ = writeln!(
        s,
        "Debug id: {}",
        super::image_identity::image_debug_id()
    );
    let _ = writeln!(s, "Symbols: {}", retained_symbols_dir().display());
    let _ = writeln!(s, "Platform: {}", platform_name());
    let _ = writeln!(s, "Hostname: {}", hostname());
    let _ = writeln!(s, "PID: {}", std::process::id());
    s
}

/// Where `build.sh dev` retains this build's symbol file. A dev PDB is otherwise
/// overwritten by the next link, which is how the 2026-09-05 dump became
/// unsymbolizable nine minutes after it was written.
///
/// Keyed by the image's own debug identity, never by a commit: a build sha names
/// the source, and the same source relinks to a different symbol file every time.
pub fn retained_symbols_dir() -> PathBuf {
    let dir = crate::paths::data_dir().join("pdb");
    match super::image_identity::debug_key() {
        Some(key) => dir.join(key),
        None => dir,
    }
}

fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

fn hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".into())
}

pub fn log_tail(n_lines: usize) -> String {
    let path = crate::paths::data_dir().join("logs").join("codeterm.log");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return format!("(log file not found at {})", path.display());
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n_lines);
    lines[start..].join("\n")
}
