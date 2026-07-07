//! Manual mesh on/off — the persisted local switch the sidebar toggle drives.
//!
//! OFF = go offline: stop advertising/broadcasting + mark peers offline so the
//! UI reflects it immediately. ON = resume: re-arm listeners, refresh addresses,
//! beacon + announce so peers see us again. The periodic loops also re-read
//! `config.mesh.enabled` each tick, so this only handles the edge transition.

use super::ports::MeshEventSink;
use super::{broadcast_announce, discovery, MeshState};

/// Runtime side-effects for a mesh enabled/disabled transition. Config persistence
/// and `config-changed` emission are owned by the app wrapper.
pub fn apply_mesh_enabled(mesh: &MeshState, enabled: bool, sink: &dyn MeshEventSink) {
    if enabled {
        log::info!("[mesh] enabled — resuming announce/advertise");
        if let Err(e) = discovery::start_listeners(mesh.clone()) {
            log::error!("[mesh] resume listeners failed: {}", e);
        }
        discovery::detect_tailscale(mesh);
        discovery::detect_local_ip(mesh);
        discovery::send_immediate_beacon(mesh);
        let mesh_arc = mesh.clone();
        tokio::spawn(async move {
            let _ = broadcast_announce(&mesh_arc, None).await;
        });
    } else {
        log::info!("[mesh] disabled — going offline, marking peers offline");
        discovery::stop_broadcast(mesh);
        let mut m = mesh.lock();
        for peer in m.paired_peers.values_mut() {
            peer.online = false;
        }
        m.discovered_peers.clear();
    }

    sink.emit_mesh_state_changed();
}

/// Apply a manual enabled/disabled change: persist config, drive the runtime
/// side-effects, and notify every client. Returns the resulting enabled state.
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
        let sink = super::app_ports::AppMeshEventSink { app };
        sink.emit_config_changed(serde_json::to_value(&snapshot).unwrap_or_default());
    }

    let sink = super::app_ports::AppMeshEventSink { app };
    apply_mesh_enabled(mesh, enabled, &sink);
    enabled
}
