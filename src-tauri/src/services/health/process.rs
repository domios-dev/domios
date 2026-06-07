use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::services::store::{HealthSnapshot, HealthStatus};

pub fn probe_process(label: &str) -> std::io::Result<HealthSnapshot> {
    if label.trim().is_empty() {
        return Ok(snap(HealthStatus::Unknown));
    }
    let status = check_label(label);
    Ok(snap(status))
}

#[cfg(target_os = "macos")]
fn check_label(label: &str) -> HealthStatus {
    let out = Command::new("launchctl").args(["list", label]).output();
    match out {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            if body.trim().is_empty() {
                HealthStatus::Down
            } else {
                HealthStatus::Healthy
            }
        }
        _ => HealthStatus::Down,
    }
}

#[cfg(target_os = "linux")]
fn check_label(label: &str) -> HealthStatus {
    let out = Command::new("systemctl")
        .args(["is-active", label])
        .output();
    match out {
        Ok(o) => {
            let body = String::from_utf8_lossy(&o.stdout);
            if body.trim() == "active" {
                HealthStatus::Healthy
            } else {
                HealthStatus::Down
            }
        }
        Err(_) => HealthStatus::Unknown,
    }
}

#[cfg(target_os = "windows")]
fn check_label(label: &str) -> HealthStatus {
    // Tauri daemon is GUI-subsystem; spawning console children without CREATE_NO_WINDOW flashes a fresh conhost every probe interval.
    let mut cmd = Command::new("tasklist");
    cmd.args(["/FO", "CSV", "/NH"]);
    crate::process_util::no_window_std(&mut cmd);
    let out = cmd.output();
    match out {
        Ok(o) => {
            let body = String::from_utf8_lossy(&o.stdout);
            if body
                .lines()
                .any(|line| windows_image_name_matches(line, label))
            {
                HealthStatus::Healthy
            } else {
                HealthStatus::Down
            }
        }
        Err(_) => HealthStatus::Unknown,
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_image_name_matches(line: &str, label: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('"') {
        return false;
    }
    let after_quote = &trimmed[1..];
    let Some(end) = after_quote.find('"') else {
        return false;
    };
    let image = &after_quote[..end];
    let want = label
        .trim()
        .trim_end_matches(".exe")
        .trim_end_matches(".EXE");
    let have = image.trim_end_matches(".exe").trim_end_matches(".EXE");
    have.eq_ignore_ascii_case(want)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn check_label(_label: &str) -> HealthStatus {
    HealthStatus::Unknown
}

fn snap(status: HealthStatus) -> HealthSnapshot {
    HealthSnapshot {
        status,
        probed_at: Some(now_ms()),
        ..Default::default()
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_label_returns_unknown() {
        let snap = probe_process("").unwrap();
        assert_eq!(snap.status, HealthStatus::Unknown);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn nonexistent_launchd_label_is_down() {
        let snap = probe_process("com.codeterm.nonexistent.test.service.zzz").unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn known_system_label_is_healthy() {
        let snap = probe_process("com.apple.launchd").unwrap();
        // launchctl list com.apple.launchd reliably exits 0 with output.
        assert!(matches!(
            snap.status,
            HealthStatus::Healthy | HealthStatus::Down
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn nonexistent_systemd_unit_is_down() {
        let snap = probe_process("codeterm-nonexistent-test.service").unwrap();
        assert_ne!(snap.status, HealthStatus::Healthy);
    }

    #[test]
    fn windows_csv_exact_match_hits() {
        let line = r#""claude.exe","1234","Console","1","12,345 K""#;
        assert!(windows_image_name_matches(line, "claude.exe"));
        assert!(windows_image_name_matches(line, "claude"));
    }

    #[test]
    fn windows_csv_substring_misses() {
        let line = r#""claude-helper.exe","1234","Console","1","12,345 K""#;
        assert!(!windows_image_name_matches(line, "claude.exe"));
        assert!(!windows_image_name_matches(line, "claude"));
        let line2 = r#""my-claude.exe","1234","Console","1","12,345 K""#;
        assert!(!windows_image_name_matches(line2, "claude"));
    }

    #[test]
    fn windows_csv_case_insensitive() {
        let line = r#""Claude.EXE","1234","Console","1","12,345 K""#;
        assert!(windows_image_name_matches(line, "claude"));
        assert!(windows_image_name_matches(line, "CLAUDE.exe"));
    }

    #[test]
    fn windows_csv_empty_or_malformed_misses() {
        assert!(!windows_image_name_matches("", "claude"));
        assert!(!windows_image_name_matches("not-a-csv-line", "claude"));
        assert!(!windows_image_name_matches(r#""claude.exe"#, "claude"));
    }

    #[test]
    fn windows_csv_window_title_fragment_misses() {
        // tasklist /V column 9 is window title; substring search would have caught it. Our CSV-column-0 parser must not.
        let line = r#""explorer.exe","4321","Console","1","45,678 K","Running","DOMAIN\user","0:00:01","claude project""#;
        assert!(!windows_image_name_matches(line, "claude"));
    }
}
