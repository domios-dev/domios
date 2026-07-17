//! App shim — domain logic lives in `codeterm-mesh`.

mod app_ports;

use std::future::Future;

pub use codeterm_mesh::*;

use app_ports::AppMeshEventSink;

/// Persist config + drive mesh runtime for the sidebar toggle.
pub fn set_mesh_enabled(
    mesh: &MeshState,
    config: &crate::config::ConfigState,
    app: &tauri::AppHandle,
    enabled: bool,
) -> bool {
    let changed = {
        let mut cfg = config.lock();
        let was = cfg.mesh.enabled;
        cfg.mesh.enabled = enabled;
        was != enabled
    };
    if changed {
        let snapshot = config.lock().clone();
        if let Err(e) = crate::config::save_config_pub(&snapshot) {
            log::error!("[mesh] failed to persist mesh.enabled: {}", e);
        }
        let sink = AppMeshEventSink { app };
        sink.emit_config_changed(serde_json::to_value(&snapshot).unwrap_or_default());
    }

    let sink = AppMeshEventSink { app };
    codeterm_mesh::apply_mesh_enabled(mesh, enabled, &sink, schedule_mesh_announce);
    enabled
}

fn schedule_mesh_announce(mesh: codeterm_mesh::MeshState) {
    spawn_on_app_runtime(codeterm_mesh::announce_enabled_mesh(mesh));
}

fn spawn_on_app_runtime<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tauri::async_runtime::spawn(future);
}

pub use app_ports::UserLastActivityProvider;

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::spawn_on_app_runtime;

    #[test]
    fn app_runtime_spawner_is_safe_without_an_ambient_tokio_runtime() {
        let (sent, received) = mpsc::channel();
        let result = std::thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            spawn_on_app_runtime(async move {
                sent.send(()).expect("test receiver remains available");
            });
        })
        .join();

        assert!(
            result.is_ok(),
            "app runtime spawn must not panic off-runtime"
        );
        received
            .recv_timeout(Duration::from_secs(1))
            .expect("app runtime must run scheduled task");
    }
}
