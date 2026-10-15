use std::fmt::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeAction {
    None,
    MarkHealthy,
    MarkUnhealthyAndCapture,
}

#[derive(Debug, Default)]
pub struct ProbeState {
    consecutive_failures: u8,
    captured_current_outage: bool,
}

#[derive(Debug, Default)]
pub struct RecoveryResyncClaims {
    announced_transition: u64,
}

impl RecoveryResyncClaims {
    pub fn claim(&mut self, transition_id: u64) -> bool {
        if transition_id <= self.announced_transition {
            return false;
        }
        self.announced_transition = transition_id;
        true
    }
}

impl ProbeState {
    pub fn record(&mut self, healthy: bool, threshold: u8) -> ProbeAction {
        if healthy {
            self.consecutive_failures = 0;
            let was_captured = std::mem::take(&mut self.captured_current_outage);
            return if was_captured {
                ProbeAction::MarkHealthy
            } else {
                ProbeAction::None
            };
        }

        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= threshold && !self.captured_current_outage {
            self.captured_current_outage = true;
            ProbeAction::MarkUnhealthyAndCapture
        } else {
            ProbeAction::None
        }
    }
}

pub fn write_liveness_report_with(
    crashes_dir: &Path,
    pid: u32,
    snapshots: &[(String, String)],
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(crashes_dir).map_err(|e| e.to_string())?;
    let path = crashes_dir.join(format!(
        "crash-{}-liveness.txt",
        crate::crash::report::timestamp_slug()
    ));
    let mut s = crate::crash::report::header("liveness");
    let _ = writeln!(s);
    let _ = writeln!(s, "Local REST API failed repeated self-health probes.");
    let _ = writeln!(s, "Probe PID: {}", pid);
    let _ = writeln!(s);
    for (name, body) in snapshots {
        let _ = writeln!(s, "== {} ==", name);
        let _ = writeln!(s, "{}", body.trim_end());
        let _ = writeln!(s);
    }
    let _ = writeln!(s, "Recent log tail (last 200 lines):");
    let _ = writeln!(s, "---");
    let _ = writeln!(s, "{}", crate::crash::report::log_tail(200));

    std::fs::write(&path, s).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn write_liveness_report() -> Result<PathBuf, String> {
    let pid = std::process::id();
    write_liveness_report_with(
        &crate::crash::report::crashes_dir(),
        pid,
        &capture_process_snapshots(pid),
    )
}

fn capture_process_snapshots(pid: u32) -> Vec<(String, String)> {
    let mut out = Vec::new();
    out.push((
        "ps".into(),
        run_capture(
            "ps",
            &[
                "-p",
                &pid.to_string(),
                "-o",
                "pid,ppid,stat,etime,lstart,command",
            ],
        ),
    ));

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        out.push((
            "lsof".into(),
            run_capture("lsof", &["-nP", "-p", &pid.to_string()]),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let path = crate::crash::report::crashes_dir().join(format!(
            "spindump-{}-{}.txt",
            pid,
            crate::crash::report::timestamp_slug()
        ));
        let body = run_capture(
            "spindump",
            &[&pid.to_string(), "5", "-file", &path.to_string_lossy()],
        );
        out.push((
            "spindump".into(),
            format!("{}\nfile={}", body, path.display()),
        ));
    }

    #[cfg(target_os = "windows")]
    {
        out.push((
            "powershell Get-Process".into(),
            run_capture(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    &format!("Get-Process -Id {} | Format-List *", pid),
                ],
            ),
        ));
    }

    out
}

fn run_capture(cmd: &str, args: &[&str]) -> String {
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    crate::process_util::no_window_std(&mut command);
    match command.output() {
        Ok(output) => {
            let mut s = String::new();
            if !output.stdout.is_empty() {
                s.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                if !s.is_empty() {
                    s.push('\n');
                }
                s.push_str("stderr:\n");
                s.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            if s.is_empty() {
                format!("(no output, status={})", output.status)
            } else {
                s
            }
        }
        Err(e) => format!("failed to run {}: {}", cmd, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_state_captures_once_per_outage_and_recovers() {
        let mut s = ProbeState::default();
        assert_eq!(s.record(false, 3), ProbeAction::None);
        assert_eq!(s.record(false, 3), ProbeAction::None);
        assert_eq!(s.record(false, 3), ProbeAction::MarkUnhealthyAndCapture);
        assert_eq!(s.record(false, 3), ProbeAction::None);
        assert_eq!(s.record(true, 3), ProbeAction::MarkHealthy);
        assert_eq!(s.record(true, 3), ProbeAction::None);
        assert_eq!(s.record(false, 3), ProbeAction::None);
        assert_eq!(s.record(false, 3), ProbeAction::None);
        assert_eq!(s.record(false, 3), ProbeAction::MarkUnhealthyAndCapture);
    }

    #[test]
    fn liveness_report_contains_injected_process_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_liveness_report_with(
            dir.path(),
            42,
            &[
                ("lsof".into(), "COMMAND PID\ncodeterm 42".into()),
                ("spindump".into(), "sample stack".into()),
            ],
        )
        .unwrap();
        let body = std::fs::read_to_string(path).unwrap();

        assert!(body.contains("Type: liveness"));
        assert!(body.contains("Probe PID: 42"));
        assert!(body.contains("== lsof =="));
        assert!(body.contains("== spindump =="));
    }
}
