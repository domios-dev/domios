#[cfg(test)]
use codeterm_crash::liveness::write_liveness_report_with;
pub(crate) use codeterm_crash::liveness::{
    write_liveness_report, ProbeAction, ProbeState, RecoveryResyncClaims,
};
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn apply_mesh_local_health(mesh: &crate::MeshStateHandle, healthy: bool) -> bool {
    mesh.lock().set_local_http_healthy(healthy)
}

pub(crate) fn handle_probe_action<F>(
    mesh: &crate::MeshStateHandle,
    action: ProbeAction,
    shutting_down: bool,
    mut write_report: F,
) -> (bool, Option<Result<PathBuf, String>>)
where
    F: FnMut() -> Result<PathBuf, String>,
{
    // expected, not a crash — never flip mesh health or write a report then.
    if shutting_down {
        return (false, None);
    }
    match action {
        ProbeAction::None => (false, None),
        ProbeAction::MarkHealthy => (apply_mesh_local_health(mesh, true), None),
        ProbeAction::MarkUnhealthyAndCapture => {
            let changed = apply_mesh_local_health(mesh, false);
            (changed, Some(write_report()))
        }
    }
}

async fn probe_local_health(client: &reqwest::Client, api_port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/api/health", api_port);
    matches!(
        client.get(url).send().await,
        Ok(resp) if resp.status().is_success()
    )
}

async fn publish_api_recovery(
    ui: &dyn crate::ports::host::UiEvents,
    mesh: &crate::MeshStateHandle,
    recovery: &crate::startup::RestApiRecovery,
) {
    apply_mesh_local_health(mesh, true);
    ui.emit("mesh-state-changed", serde_json::Value::Null);
    let report = crate::mesh::broadcast_announce(mesh, None).await;
    log::info!(
        "[liveness] local API generation {} recovery transition {} announced; sent={} ok={} errors={}",
        recovery.status.generation,
        recovery.transition_id,
        report.sent,
        report.ok,
        report.errors.len()
    );
}

pub fn start_api_liveness_watchdog(
    ui: std::sync::Arc<dyn crate::ports::host::UiEvents>,
    mesh: crate::MeshStateHandle,
    rest_api: crate::startup::RestApiControlState,
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
                let mut recovery_events = rest_api
                    .take_recovery_events()
                    .expect("REST API recovery consumer already installed");
                let mut recovery_claims = RecoveryResyncClaims::default();
                loop {
                    let recovery = tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(30)) => Some(None),
                        recovery = recovery_events.recv() => recovery.map(Some),
                    };
                    let Some(recovery) = recovery else {
                        break;
                    };
                    // unhealthy from here on is the clean quit, not an outage.
                    if crate::crash::shutdown::is_shutting_down() {
                        log::info!("[liveness] shutting down; watchdog stopping");
                        break;
                    }
                    if let Some(recovery) = recovery {
                        if recovery_claims.claim(recovery.transition_id) {
                            publish_api_recovery(ui.as_ref(), &mesh, &recovery).await;
                        }
                        continue;
                    }
                    let healthy = probe_local_health(&client, api_port).await;
                    if healthy {
                        rest_api.record_successful_probe();
                    }
                    let action = state.record(healthy, 3);
                    match action {
                        ProbeAction::None => {}
                        ProbeAction::MarkHealthy => {
                            rest_api.record_successful_probe();
                        }
                        ProbeAction::MarkUnhealthyAndCapture => {
                            let diagnostic = rest_api.record_unanswered_probe();
                            log::error!(
                                "[liveness] local API diagnostic classification={diagnostic:?}"
                            );
                            // await, after the loop-top guard.
                            let (changed, report_result) = handle_probe_action(
                                &mesh,
                                action,
                                crate::crash::shutdown::is_shutting_down(),
                                write_liveness_report,
                            );
                            if changed {
                                ui.emit("mesh-state-changed", serde_json::Value::Null);
                            }
                            match report_result {
                                Some(Ok(path)) => log::error!(
                                    "[liveness] local API unhealthy; report written to {}",
                                    path.display()
                                ),
                                Some(Err(e)) => {
                                    log::error!("[liveness] report write failed: {}", e)
                                }
                                None => log::info!(
                                    "[liveness] unhealthy probe suppressed (shutting down)"
                                ),
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
            handle_probe_action(&mesh, ProbeAction::MarkUnhealthyAndCapture, false, || {
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
    fn handle_probe_action_skips_write_when_shutting_down() {
        let mesh = std::sync::Arc::new(parking_lot::Mutex::new(crate::mesh::MeshManager::new(
            "test".into(),
            7685,
        )));
        {
            let mut m = mesh.lock();
            m.disk_persist = false;
            m.tailscale_ip = Some("100.64.1.2".into());
            // An address is advertised only where a listener is ready, so a
            // configured tailnet ip alone announces nothing.
            m.ready_listeners = vec!["100.64.1.2:7685".parse().unwrap()];
        }

        let mut write_called = false;
        let (changed, report) = handle_probe_action(
            &mesh,
            ProbeAction::MarkUnhealthyAndCapture,
            true, // shutting down — a clean quit's API-down is not a crash
            || {
                write_called = true;
                Err("write_report must not run during shutdown".into())
            },
        );

        assert!(!changed, "mesh health must not flip during shutdown");
        assert!(report.is_none(), "no report attempted during shutdown");
        assert!(!write_called, "write_report closure must not be invoked");
        // Mesh stays online: we never marked it unhealthy.
        let ann = mesh.lock().build_announce();
        assert!(ann.online);
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
