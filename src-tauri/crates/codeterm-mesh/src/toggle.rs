//! Manual mesh on/off runtime — config persistence stays in the app wrapper.

use super::ports::MeshEventSink;
use super::{broadcast_announce, discovery, MeshState};

/// Runtime side-effects for a mesh enabled/disabled transition.
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
