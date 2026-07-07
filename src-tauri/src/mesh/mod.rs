//! App shim — domain logic lives in `codeterm-mesh`.

mod app_ports;

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
    codeterm_mesh::apply_mesh_enabled(mesh, enabled, &sink);
    enabled
}

pub use app_ports::UserLastActivityProvider;
