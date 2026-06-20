//! Startup reaper for orphaned WebView2 children (`msedgewebview2.exe`) left
//! behind by a hard GUI crash (GH #183). A WebView2 fault kills the single
//! Tauri process; its `msedgewebview2.exe` browser/GPU/renderer children
//! survive the dead parent and block a clean relaunch. Modeled on
//! `keepawake.rs`: journal the owning PID while alive, enumerate + kill
//! survivors at startup, sentinel-gated so a healthy concurrent instance is
//! never touched.
//!
//! Safety: we reap ONLY `msedgewebview2.exe` whose parent is the exact PID we
//! journaled from a prior run, and only when that parent PID is verifiably
//! dead and the last exit was not clean. The journal + decision logic is
//! cross-platform and unit-tested; the enumerate + kill is Windows-only.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const JOURNAL_FILE: &str = "webview2-journal.json";
const WEBVIEW2_IMAGE: &str = "msedgewebview2.exe";

/// Records the CodeTerm process that owns the live WebView2 children, so the
/// next launch can reap them if this process dies hard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Webview2Journal {
    owner_pid: u32,
    started_at_s: i64,
}

/// One enumerated process row (Windows `Win32_Process` projection).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcInfo {
    pid: u32,
    ppid: u32,
    image: String,
}

fn journal_path() -> PathBuf {
    crate::paths::data_dir().join(JOURNAL_FILE)
}

fn load_journal_from(path: &Path) -> Option<Webview2Journal> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<Webview2Journal>(&bytes).ok()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn save_journal_to(path: &Path, journal: &Webview2Journal) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let bytes = match serde_json::to_vec(journal) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[webview2-reap] failed to serialize journal: {}", e);
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, bytes) {
        log::warn!("[webview2-reap] failed to write journal tmp: {}", e);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        log::warn!("[webview2-reap] failed to rename journal: {}", e);
        let _ = std::fs::remove_file(&tmp);
    }
}

fn clear_journal_at(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Reap only when we own a prior journal AND the last exit was not clean. A
/// `.clean_exit` recorded at/after the journal was written means the prior run
/// shut down gracefully — its WebView2 children are already gone, nothing to do.
fn should_reap(journal: Option<&Webview2Journal>, clean_exit_ts: Option<i64>) -> bool {
    let Some(j) = journal else {
        return false;
    };
    !matches!(clean_exit_ts, Some(ts) if ts >= j.started_at_s)
}

/// Survivors to kill: exact WebView2 image, parented to our journaled owner,
/// and only when that owner PID is truly dead (a live owner means it is either
/// still running or its PID was recycled — never risk a healthy instance).
#[cfg_attr(not(windows), allow(dead_code))]
fn select_webview2_to_kill(owner_pid: u32, owner_alive: bool, candidates: &[ProcInfo]) -> Vec<u32> {
    if owner_alive {
        return Vec::new();
    }
    candidates
        .iter()
        .filter(|p| p.ppid == owner_pid && p.image.eq_ignore_ascii_case(WEBVIEW2_IMAGE))
        .map(|p| p.pid)
        .collect()
}

fn now_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Startup entry: reap any WebView2 orphans left by a hard crash of a prior
/// run, then journal THIS process so the next launch can reap us if we crash.
/// Call before window init. No-op on non-Windows (WebView2 is Windows-only).
pub(crate) fn reap_orphaned_webview2() {
    let path = journal_path();
    let prior = load_journal_from(&path);
    let clean_exit_ts = crate::crash::shutdown::clean_exit_timestamp();

    if should_reap(prior.as_ref(), clean_exit_ts) {
        if let Some(j) = prior.as_ref() {
            reap_for_owner(j.owner_pid);
        }
    }

    journal_current_process(&path);
}

/// Clear the journal on a clean exit so the next launch has nothing to reap.
pub(crate) fn clear_journal_for_exit() {
    clear_journal_at(&journal_path());
}

#[cfg(windows)]
fn journal_current_process(path: &Path) {
    save_journal_to(
        path,
        &Webview2Journal {
            owner_pid: std::process::id(),
            started_at_s: now_s(),
        },
    );
}

/// On non-Windows there are no WebView2 children to track — don't write a file.
#[cfg(not(windows))]
fn journal_current_process(_path: &Path) {
    let _ = now_s;
}

#[cfg(windows)]
fn reap_for_owner(owner_pid: u32) {
    let owner_alive = process_exists(owner_pid);
    let candidates = enumerate_webview2();
    let victims = select_webview2_to_kill(owner_pid, owner_alive, &candidates);
    if victims.is_empty() {
        if owner_alive {
            log::info!(
                "[webview2-reap] prior owner pid {} still present (alive/recycled) — skipping reap",
                owner_pid
            );
        }
        return;
    }
    let mut killed = Vec::new();
    for pid in victims {
        match terminate_pid(pid) {
            Ok(()) => killed.push(pid),
            Err(e) => log::warn!("[webview2-reap] failed to terminate pid {}: {}", pid, e),
        }
    }
    if !killed.is_empty() {
        log::warn!(
            "[webview2-reap] reaped orphaned msedgewebview2 pid(s) of dead owner {}: {:?}",
            owner_pid,
            killed
        );
    }
}

#[cfg(not(windows))]
fn reap_for_owner(_owner_pid: u32) {}

#[cfg(windows)]
fn enumerate_webview2() -> Vec<ProcInfo> {
    let script = format!(
        "Get-CimInstance Win32_Process -Filter \"Name='{}'\" | \
         Select-Object Name,ProcessId,ParentProcessId | ConvertTo-Csv -NoTypeInformation",
        WEBVIEW2_IMAGE
    );
    let output = match std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            log::warn!(
                "[webview2-reap] enumerate exited with {}: {}",
                o.status,
                String::from_utf8_lossy(&o.stderr)
            );
            return Vec::new();
        }
        Err(e) => {
            log::warn!("[webview2-reap] enumerate failed: {}", e);
            return Vec::new();
        }
    };
    parse_process_csv(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    let script = format!(
        "Get-CimInstance Win32_Process -Filter \"ProcessId={}\" | \
         Select-Object ProcessId | ConvertTo-Csv -NoTypeInformation",
        pid
    );
    let output = match std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
    {
        Ok(o) if o.status.success() => o,
        // On any error, assume alive — never reap on an ambiguous liveness check.
        _ => return true,
    };
    // Header line only (no data rows) → no such process.
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
        > 1
}

/// Parse `ConvertTo-Csv` output of `Name,ProcessId,ParentProcessId`. The first
/// non-empty line is the header; each value is double-quoted.
#[cfg(windows)]
fn parse_process_csv(stdout: &str) -> Vec<ProcInfo> {
    let mut rows = stdout.lines().filter(|l| !l.trim().is_empty());
    let _header = rows.next();
    rows.filter_map(|line| {
        let cols: Vec<String> = line.split(',').map(unquote_csv).collect();
        if cols.len() < 3 {
            return None;
        }
        Some(ProcInfo {
            image: cols[0].clone(),
            pid: cols[1].parse().ok()?,
            ppid: cols[2].parse().ok()?,
        })
    })
    .collect()
}

#[cfg(windows)]
fn unquote_csv(field: &str) -> String {
    field.trim().trim_matches('"').to_string()
}

#[cfg(windows)]
fn terminate_pid(pid: u32) -> Result<(), String> {
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill exited with {}", status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(owner_pid: u32, started_at_s: i64) -> Webview2Journal {
        Webview2Journal {
            owner_pid,
            started_at_s,
        }
    }

    fn proc(pid: u32, ppid: u32, image: &str) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            image: image.to_string(),
        }
    }

    #[test]
    fn journal_round_trips_and_clear_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_FILE);
        let j = journal(4242, 1_700_000_000);

        save_journal_to(&path, &j);
        assert_eq!(load_journal_from(&path), Some(j));

        clear_journal_at(&path);
        assert!(!path.exists());
        assert_eq!(load_journal_from(&path), None);
    }

    #[test]
    fn skip_reap_when_no_journal() {
        assert!(!should_reap(None, None));
        assert!(!should_reap(None, Some(1_700_000_000)));
    }

    #[test]
    fn skip_reap_when_clean_exit_after_journal() {
        let j = journal(10, 1_700_000_000);
        // Clean exit recorded at/after journal write → prior run quit gracefully.
        assert!(!should_reap(Some(&j), Some(1_700_000_000)));
        assert!(!should_reap(Some(&j), Some(1_700_000_500)));
    }

    #[test]
    fn reap_when_crash_after_journal() {
        let j = journal(10, 1_700_000_000);
        // No clean-exit sentinel, or one older than this run's journal → crash.
        assert!(should_reap(Some(&j), None));
        assert!(should_reap(Some(&j), Some(1_699_999_999)));
    }

    #[test]
    fn select_kills_only_our_dead_owners_webview2() {
        let candidates = vec![
            proc(100, 50, "msedgewebview2.exe"),  // ours
            proc(101, 50, "msedgewebview2.exe"),  // ours
            proc(102, 999, "msedgewebview2.exe"), // another instance's owner
            proc(103, 50, "chrome.exe"),          // wrong image
        ];
        let victims = select_webview2_to_kill(50, false, &candidates);
        assert_eq!(victims, vec![100, 101]);
    }

    #[test]
    fn select_skips_everything_when_owner_alive() {
        let candidates = vec![proc(100, 50, "msedgewebview2.exe")];
        // Live/recycled owner PID → never risk killing a healthy instance.
        assert!(select_webview2_to_kill(50, true, &candidates).is_empty());
    }

    #[test]
    fn select_matches_image_case_insensitively() {
        let candidates = vec![proc(100, 50, "MsEdgeWebView2.exe")];
        assert_eq!(select_webview2_to_kill(50, false, &candidates), vec![100]);
    }

    #[cfg(not(windows))]
    #[test]
    fn reap_for_owner_is_noop_on_non_windows() {
        // Must not panic / shell out anywhere on non-Windows.
        reap_for_owner(12345);
    }

    #[cfg(not(windows))]
    #[test]
    fn journal_current_process_writes_nothing_on_non_windows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_FILE);
        journal_current_process(&path);
        assert!(!path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn parse_process_csv_reads_rows_and_skips_header() {
        let stdout = "\"Name\",\"ProcessId\",\"ParentProcessId\"\r\n\
                      \"msedgewebview2.exe\",\"1234\",\"5678\"\r\n\
                      \"msedgewebview2.exe\",\"1235\",\"5678\"\r\n";
        let rows = parse_process_csv(stdout);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], proc(1234, 5678, "msedgewebview2.exe"));
        assert_eq!(rows[1], proc(1235, 5678, "msedgewebview2.exe"));
    }
}
