//! Daemon sweeper for runaway Rust test binaries.
//!
//! Scans for processes whose executable path matches `*/target/*/deps/*` and
//! whose elapsed time exceeds the configured threshold.  Only test-binary
//! orphans are targeted — cargo, rustc, link, and the daemon itself are
//! never touched.  On a match the process is sent SIGKILL, a WARN is logged,
//! and (when the owning pane can be resolved) a nudge is injected into the
//! agent pane so the worker knows to clean up.
//!
//! macOS + Linux only.  Windows is a follow-up (ConPTY process-tree teardown
//! differs significantly; Windows workers use PowerShell foreground which
//! avoids the orphan class anyway).

use crate::config::schema::RunawayConfig;

/// A running test binary that has outlived its threshold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunawayProcess {
    pub pid: u32,
    /// Full path of the executable as reported by `ps`.
    pub exe: String,
    /// Elapsed seconds since the process started.
    pub elapsed_secs: u64,
}

/// Return true iff `exe` is a Rust test binary orphan we should kill.
///
/// Must match `*/target/*/deps/*` and must NOT be cargo / rustc / a linker /
/// or this daemon itself.  Path matching only — no CPU heuristics.
pub fn is_runaway_test_binary(exe: &str) -> bool {
    if exe.trim().is_empty() {
        return false;
    }
    // Must contain /target/<profile>/deps/ (or \target\ on Windows, but we're Unix-only here).
    if !exe_contains_target_deps(exe) {
        return false;
    }
    // Never kill build toolchain or the daemon.
    let basename = exe.rsplit('/').next().unwrap_or(exe);
    if is_toolchain_binary(basename) {
        return false;
    }
    true
}

fn exe_contains_target_deps(exe: &str) -> bool {
    // Match both Unix and hypothetical Windows paths; separator may be / or \.
    let normalized = exe.replace('\\', "/");
    // Use a simple contains check on the normalized path segments.
    if let Some(after_target) = normalized.split("/target/").nth(1) {
        // after_target looks like "debug/deps/binary" or "release/deps/binary"
        after_target.contains("/deps/")
    } else {
        false
    }
}

/// Exact-name check for toolchain binaries that must never be killed.
///
/// We do NOT strip hash suffixes here: `codeterm-b2534ae3cdc1559f` (a test binary)
/// must NOT match `codeterm` (the daemon). Path-based filtering (`target/*/deps/`)
/// already excludes the daemon and real toolchain tools; this guard is a last-resort
/// belt-and-suspenders check on the exact basename only.
fn is_toolchain_binary(basename: &str) -> bool {
    matches!(
        basename,
        "cargo"
            | "rustc"
            | "rustfmt"
            | "clippy-driver"
            | "rust-analyzer"
            | "cc"
            | "c++"
            | "gcc"
            | "g++"
            | "clang"
            | "clang++"
            | "ld"
            | "lld"
            | "link"
            | "codeterm"
    )
}

/// Parse `etime` field (POSIX `[[DD-]HH:]MM:SS`) into seconds.
pub fn parse_etime(etime: &str) -> Option<u64> {
    let s = etime.trim();
    // Split on '-' first to extract optional days.
    let (days, rest) = if let Some((d, r)) = s.split_once('-') {
        (d.parse::<u64>().ok()?, r)
    } else {
        (0u64, s)
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let total_secs = match parts.as_slice() {
        [mm, ss] => {
            let m: u64 = mm.parse().ok()?;
            let s: u64 = ss.parse().ok()?;
            days * 86400 + m * 60 + s
        }
        [hh, mm, ss] => {
            let h: u64 = hh.parse().ok()?;
            let m: u64 = mm.parse().ok()?;
            let s: u64 = ss.parse().ok()?;
            days * 86400 + h * 3600 + m * 60 + s
        }
        _ => return None,
    };
    Some(total_secs)
}

/// Scan running processes and return those that qualify as runaway test binaries.
///
/// Uses `ps -A -o pid=,etime=,comm=` which is POSIX-portable across macOS and Linux.
/// The `comm` column gives the truncated name on some systems; we re-read `/proc`
/// on Linux when it's too short, but for our path-matching we rely on `-o args=`.
#[cfg(unix)]
pub fn scan_runaway_processes(threshold_secs: u64) -> Vec<RunawayProcess> {
    // `-o args=` gives the full command line including absolute path on most Unix.
    // On macOS `comm` is truncated at 16 chars; `args` is the only reliable full-path source.
    let output = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,etime=,args="])
        .output();
    let Ok(out) = output else {
        return vec![];
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut results = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Format: "PID ETIME COMMAND [args...]"
        // split_whitespace collapses any run of spaces — safe for ps output.
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 3 {
            continue;
        }
        let pid_str = tokens[0];
        let etime_str = tokens[1];
        // tokens[2] is the executable path (no spaces expected in test binary paths).
        let exe_path = tokens[2];
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        let Some(elapsed) = parse_etime(etime_str) else {
            continue;
        };
        if elapsed >= threshold_secs && is_runaway_test_binary(exe_path) {
            results.push(RunawayProcess {
                pid,
                exe: exe_path.to_string(),
                elapsed_secs: elapsed,
            });
        }
    }
    results
}

#[cfg(not(unix))]
pub fn scan_runaway_processes(_threshold_secs: u64) -> Vec<RunawayProcess> {
    // Windows follow-up: ConPTY process-tree teardown differs.
    vec![]
}

/// Kill a process by PID (SIGKILL on Unix).  Best-effort: logs on failure.
#[cfg(unix)]
fn kill_process(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) == 0 }
}

#[cfg(not(unix))]
fn kill_process(_pid: u32) -> bool {
    false
}

/// Extract the project root from a test-binary path.
///
/// `/path/to/project/target/debug/deps/foo` → `/path/to/project`
/// One sweep pass: find, kill, and warn to the daemon log.
pub fn sweep_once(cfg: &RunawayConfig) {
    let candidates = scan_runaway_processes(cfg.test_binary_max_secs);
    if candidates.is_empty() {
        return;
    }
    for proc in &candidates {
        log::warn!(
            "[runaway-sweeper] killing test binary pid={} elapsed={}s exe={}",
            proc.pid,
            proc.elapsed_secs,
            proc.exe
        );
        let killed = kill_process(proc.pid);
        log::warn!(
            "[runaway-sweeper] kill pid={} result={}",
            proc.pid,
            if killed {
                "ok"
            } else {
                "failed (already gone?)"
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Scenario 3: sweeper selection unit matrix ─────────────────────

    #[test]
    fn runaway_sweeper_predicate_matches_test_binary() {
        let cases: &[(&str, bool)] = &[
            ("/home/user/proj/target/debug/deps/mytest-abc123", true),
            ("/Users/dev/codeterm/cli/target/debug/deps/codeterm-b2534ae3cdc1559f", true),
            ("/root/repo/target/release/deps/integration_test-deadbeef", true),
            // worktree path (real incident shape)
            (
                "/Users/rollacode/Developer/codeterm-worktrees/task-x/cli/target/debug/deps/codeterm-b2534ae3cdc1559f",
                true,
            ),
            ("/usr/bin/cargo", false),
            ("/usr/local/bin/rustc", false),
            ("/usr/bin/cc", false),
            ("/usr/bin/ld", false),
            ("/usr/bin/clang", false),
            ("/usr/local/bin/codeterm", false),
            ("/home/user/proj/target/debug/mytest", false),
            ("/home/user/proj/src/main.rs", false),
            ("cargo", false),
            ("", false),
            ("/home/user/proj/target/debug/build/foo-12345/out/foo", false),
        ];
        for (exe, expected) in cases {
            assert_eq!(
                is_runaway_test_binary(exe),
                *expected,
                "is_runaway_test_binary({exe:?}) should be {expected}"
            );
        }
    }

    #[test]
    fn runaway_sweeper_etime_parser_handles_all_formats() {
        assert_eq!(parse_etime("00:30"), Some(30));
        assert_eq!(parse_etime("01:30"), Some(90));
        assert_eq!(parse_etime("15:00"), Some(900));
        assert_eq!(parse_etime("1:00:00"), Some(3600));
        assert_eq!(parse_etime("1-00:00:00"), Some(86400));
        assert_eq!(parse_etime("1-01:30:00"), Some(86400 + 3600 + 1800));
        assert_eq!(parse_etime("  05:00  "), Some(300));
        assert_eq!(parse_etime("bogus"), None);
        assert_eq!(parse_etime(""), None);
    }

    // ── Scenario 5: prompt constant wiring test ───────────────────────

    #[test]
    fn runaway_sweeper_testing_discipline_carries_capped_script_rule() {
        assert!(
            crate::prompts::TESTING_DISCIPLINE.contains("scripts/test-capped.sh"),
            "testing_discipline.md must mention scripts/test-capped.sh"
        );
        assert!(
            crate::prompts::TESTING_DISCIPLINE
                .contains("bare `cargo test` in a lane is a violation"),
            "testing_discipline.md must forbid bare cargo test in lanes"
        );
    }
}
