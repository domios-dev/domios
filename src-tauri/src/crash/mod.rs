//! Crash reporting — panic hook (Rust) + native-crash handler (SIGSEGV/SIGABRT/SEH).
//!
//! Reports land in `~/.codeterm/crashes/` (dev: `~/.codeterm-dev/crashes/`):
//!   crash-<ts>-panic.txt  — Rust panic (always readable)
//!   crash-<ts>-native.dmp — binary minidump from crash-handler
//!   crash-<ts>-native.txt — readable sidecar for the minidump
//!
//! Read a .dmp with `scripts/symbolicate-minidump.py` (Windows, no install
//! needed); elsewhere use `minidump_stackwalk` with `dump_syms` symbols.

pub mod dump_record;
pub mod exit;
pub mod image_identity;
pub mod lifecycle;
#[cfg(target_os = "macos")]
pub mod macos_quit;
mod native;
mod panic;
pub mod report;
pub mod restore_report;
pub mod shutdown;

use std::path::{Path, PathBuf};

/// Install panic + native handlers. Call once, very early in `run()` before
/// any thread spawn. The native `CrashHandler` is leaked to keep it alive for
/// the lifetime of the process.
pub fn install() {
    // Resolved here so the crash handler only formats a cached string — a handler
    // is no place to open and parse the executable.
    let _ = image_identity::image_debug_id();
    let _ = image_identity::image_path();
    panic::install_panic_hook();
    if let Some(h) = native::install() {
        Box::leak(Box::new(h));
    }
}

pub fn record_startup() -> lifecycle::RecoveryState {
    lifecycle::record_startup()
}

pub fn record_clean_shutdown(reason: lifecycle::ShutdownReason) {
    lifecycle::record_clean_shutdown(reason);
}

#[tauri::command]
pub fn get_recovery_state() -> lifecycle::RecoveryState {
    lifecycle::recovery_state()
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
/// Reports written at or before the last graceful exit (the `.clean_exit`
/// sentinel) are suppressed — a clean quit's API-down liveness report is not a
/// crash. Updates the marker after a successful scan.
pub fn check_previous_crashes() -> Vec<PathBuf> {
    let dir = report::crashes_dir();
    let marker = crate::paths::data_dir()
        .join("state")
        .join("crashes_last_seen.txt");

    let last_seen = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0);

    let clean_exit_ts = shutdown::clean_exit_timestamp();

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut candidates: Vec<(i64, PathBuf)> = Vec::new();
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

        candidates.push((mtime, path));
    }

    let (new_reports, latest_mtime) = select_reports(candidates, last_seen, clean_exit_ts);

    if latest_mtime > last_seen {
        update_marker(&marker, latest_mtime);
    }

    new_reports
}

/// Pure selection logic, split out for testing. Returns the reports to surface
/// (sorted oldest-first) and the new marker high-water mark. A report surfaces
/// only if its mtime is newer than `last_seen` AND newer than the clean-exit
/// sentinel (when one exists). The high-water mark advances past suppressed
/// reports too, so they stay suppressed even if the sentinel is later removed.
fn select_reports(
    candidates: Vec<(i64, PathBuf)>,
    last_seen: i64,
    clean_exit_ts: Option<i64>,
) -> (Vec<PathBuf>, i64) {
    let mut new_reports: Vec<(i64, PathBuf)> = Vec::new();
    let mut latest_mtime = last_seen;

    for (mtime, path) in candidates {
        if mtime <= last_seen {
            continue;
        }
        latest_mtime = latest_mtime.max(mtime);
        if clean_exit_ts.is_some_and(|ce| mtime <= ce) {
            continue;
        }
        new_reports.push((mtime, path));
    }

    new_reports.sort_by_key(|(mtime, _)| *mtime);
    (
        new_reports.into_iter().map(|(_, p)| p).collect(),
        latest_mtime,
    )
}

fn update_marker(marker: &Path, ts: i64) {
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(marker, ts.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_reports_ignores_pre_clean_exit_reports() {
        // shutdown window) is suppressed; the genuine crash at 200 surfaces.
        let candidates = vec![
            (100, PathBuf::from("crash-old-liveness.txt")),
            (200, PathBuf::from("crash-new-native.txt")),
        ];
        let (reports, latest) = select_reports(candidates, 0, Some(150));
        assert_eq!(reports, vec![PathBuf::from("crash-new-native.txt")]);
        assert_eq!(latest, 200);
    }

    #[test]
    fn select_reports_surfaces_real_crash_without_sentinel() {
        // No sentinel (no prior clean exit) → a real report still surfaces.
        let candidates = vec![(200, PathBuf::from("crash-panic.txt"))];
        let (reports, latest) = select_reports(candidates, 0, None);
        assert_eq!(reports, vec![PathBuf::from("crash-panic.txt")]);
        assert_eq!(latest, 200);
    }

    #[test]
    fn select_reports_surfaces_real_crash_after_clean_exit() {
        // in the new run and must surface (regression guard).
        let candidates = vec![(500, PathBuf::from("crash-segv-native.txt"))];
        let (reports, _) = select_reports(candidates, 0, Some(150));
        assert_eq!(reports, vec![PathBuf::from("crash-segv-native.txt")]);
    }

    #[test]
    fn select_reports_respects_last_seen() {
        let candidates = vec![
            (50, PathBuf::from("already-seen.txt")),
            (300, PathBuf::from("fresh.txt")),
        ];
        let (reports, latest) = select_reports(candidates, 100, None);
        assert_eq!(reports, vec![PathBuf::from("fresh.txt")]);
        assert_eq!(latest, 300);
    }
}
