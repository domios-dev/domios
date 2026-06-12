//! Crash reporting — panic hook (Rust) + native-crash handler (SIGSEGV/SIGABRT/SEH).
//!
//! Reports land in `~/.codeterm/crashes/` (dev: `~/.codeterm-dev/crashes/`):
//!   crash-<ts>-panic.txt  — Rust panic (always readable)
//!   crash-<ts>-native.dmp — binary minidump from crash-handler
//!   crash-<ts>-native.txt — readable sidecar for the minidump

mod native;
mod panic;
pub mod report;

use std::path::{Path, PathBuf};

/// Install panic + native handlers. Call once, very early in `run()` before
/// any thread spawn. The native `CrashHandler` is leaked to keep it alive for
/// the lifetime of the process.
pub fn install() {
    panic::install_panic_hook();
    if let Some(h) = native::install() {
        Box::leak(Box::new(h));
    }
}

/// Trigger a synthetic crash for verifying the handlers work end-to-end.
/// Dev-only — the function literally does not exist in release builds, so
/// there is no way to reach it from a production daemon even via a forged
/// JSON-RPC method name. Pair with the socket-side `cfg(debug_assertions)`
/// gate on `debug.crash`.
#[cfg(debug_assertions)]
pub fn trigger_test_crash(kind: &str) -> Result<(), String> {
    match kind {
        "panic" => {
            std::thread::spawn(|| {
                panic!("crash-handler test: synthetic panic from debug trigger");
            });
            Ok(())
        }
        "segv" => {
            std::thread::spawn(|| {
                // SAFETY: deliberately triggering SIGSEGV for crash-handler verification.
                #[allow(deref_nullptr)]
                unsafe {
                    *(std::ptr::null_mut::<u8>()) = 1;
                }
            });
            Ok(())
        }
        "abort" => {
            std::thread::spawn(|| {
                std::process::abort();
            });
            Ok(())
        }
        other => Err(format!("unknown crash kind: {}", other)),
    }
}

/// Scan the crashes directory for reports newer than the marker file.
/// Returns absolute paths to text reports only (the human-readable artifact).
/// Updates the marker after a successful scan.
pub fn check_previous_crashes() -> Vec<PathBuf> {
    let dir = report::crashes_dir();
    let marker = crate::paths::data_dir()
        .join("state")
        .join("crashes_last_seen.txt");

    let last_seen = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0);

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut new_reports: Vec<(i64, PathBuf)> = Vec::new();
    let mut latest_mtime = last_seen;

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".txt") {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        if mtime > last_seen {
            new_reports.push((mtime, path));
            latest_mtime = latest_mtime.max(mtime);
        }
    }

    if latest_mtime > last_seen {
        update_marker(&marker, latest_mtime);
    }

    new_reports.sort_by_key(|(mtime, _)| *mtime);
    new_reports.into_iter().map(|(_, p)| p).collect()
}

fn update_marker(marker: &Path, ts: i64) {
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(marker, ts.to_string());
}
