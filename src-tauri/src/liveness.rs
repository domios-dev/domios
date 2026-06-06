//! Local daemon liveness watchdog.
//!
//! The watchdog probes the local REST API from outside the API request stack.
//! If the listener accepts TCP but handlers stop responding, this thread can
//! still mark mesh advertisement unhealthy and write a capture report.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::Emitter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeAction {
    None,
    MarkHealthy,
    MarkUnhealthyAndCapture,
}

#[derive(Debug, Default)]
pub(crate) struct ProbeState {
    consecutive_failures: u8,
    captured_current_outage: bool,
}

impl ProbeState {
    pub(crate) fn record(&mut self, healthy: bool, threshold: u8) -> ProbeAction {
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

pub(crate) fn apply_mesh_local_health(mesh: &crate::MeshStateHandle, healthy: bool) -> bool {
    mesh.lock().set_local_http_healthy(healthy)
}

pub(crate) fn handle_probe_action<F>(
    mesh: &crate::MeshStateHandle,
    action: ProbeAction,
    mut write_report: F,
) -> (bool, Option<Result<PathBuf, String>>)
where
    F: FnMut() -> Result<PathBuf, String>,
{
    match action {
        ProbeAction::None => (false, None),
        ProbeAction::MarkHealthy => (apply_mesh_local_health(mesh, true), None),
        ProbeAction::MarkUnhealthyAndCapture => {
            let changed = apply_mesh_local_health(mesh, false);
            (changed, Some(write_report()))
        }
    }
}

pub(crate) fn write_liveness_report_with(
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

pub(crate) fn write_liveness_report() -> Result<PathBuf, String> {
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
    match std::process::Command::new(cmd).args(args).output() {
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

async fn probe_local_health(client: &reqwest::Client, api_port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/api/health", api_port);
    matches!(
        client.get(url).send().await,
        Ok(resp) if resp.status().is_success()
    )
}

pub(crate) fn start_api_liveness_watchdog(
    app: tauri::AppHandle,
    mesh: crate::MeshStateHandle,
    api_port: u16,
) {
    std::thread::Builder::new()
        .name("api-liveness-watchdog".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create API liveness watchdog runtime");
            rt.block_on(async move {
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap_or_default();
                let mut state = ProbeState::default();
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let action = state.record(probe_local_health(&client, api_port).await, 3);
                    match action {
                        ProbeAction::None => {}
                        ProbeAction::MarkHealthy => {
                            let (changed, _) =
                                handle_probe_action(&mesh, action, write_liveness_report);
                            if changed {
                                let _ = app.emit("mesh-state-changed", ());
                                let report = crate::mesh::broadcast_announce(&mesh, None).await;
                                log::info!(
                                    "[liveness] local API recovered; announce sent={} ok={} errors={}",
                                    report.sent,
                                    report.ok,
                                    report.errors.len()
                                );
                            }
                        }
                        ProbeAction::MarkUnhealthyAndCapture => {
                            let (changed, report_result) =
                                handle_probe_action(&mesh, action, write_liveness_report);
                            if changed {
                                let _ = app.emit("mesh-state-changed", ());
                            }
                            match report_result.expect("unhealthy action writes a report") {
                                Ok(path) => log::error!(
                                    "[liveness] local API unhealthy; report written to {}",
                                    path.display()
                                ),
                                Err(e) => log::error!("[liveness] report write failed: {}", e),
                            }
                            let report = crate::mesh::broadcast_announce(&mesh, None).await;
                            log::warn!(
                                "[liveness] unhealthy announce sent={} ok={} errors={}",
                                report.sent,
                                report.ok,
                                report.errors.len()
                            );
                        }
                    }
                }
            });
        })
        .expect("failed to spawn API liveness watchdog");
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

    #[test]
    fn unhealthy_probe_action_flips_mesh_offline_and_writes_report() {
        let mesh = std::sync::Arc::new(parking_lot::Mutex::new(crate::mesh::MeshManager::new(
            "test".into(),
            7685,
        )));
        {
            let mut m = mesh.lock();
            m.disk_persist = false;
            m.tailscale_ip = Some("100.64.1.2".into());
        }
        let dir = tempfile::tempdir().unwrap();

        let (changed, report) =
            handle_probe_action(&mesh, ProbeAction::MarkUnhealthyAndCapture, || {
                write_liveness_report_with(dir.path(), 42, &[("lsof".into(), "codeterm 42".into())])
            });

        assert!(changed);
        let path = report.expect("report attempted").expect("report written");
        assert!(std::fs::read_to_string(path)
            .unwrap()
            .contains("== lsof =="));
        let ann = mesh.lock().build_announce();
        assert!(!ann.online);
        assert!(ann.addresses.is_empty());
    }

    #[test]
    fn mesh_health_gate_builds_offline_announce() {
        let mesh = std::sync::Arc::new(parking_lot::Mutex::new(crate::mesh::MeshManager::new(
            "test".into(),
            7685,
        )));
        {
            let mut m = mesh.lock();
            m.disk_persist = false;
            m.tailscale_ip = Some("100.64.1.2".into());
        }
        assert!(apply_mesh_local_health(&mesh, false));
        let ann = mesh.lock().build_announce();

        assert!(!ann.online);
        assert!(ann.addresses.is_empty());
    }
}
