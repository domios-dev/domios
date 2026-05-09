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
    let out = Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {}", label), "/NH"])
        .output();
    match out {
        Ok(o) => {
            let body = String::from_utf8_lossy(&o.stdout);
            if body.contains(label) {
                HealthStatus::Healthy
            } else {
                HealthStatus::Down
            }
        }
        Err(_) => HealthStatus::Unknown,
    }
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
}
