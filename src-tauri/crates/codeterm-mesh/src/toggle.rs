//! Manual mesh on/off runtime — config persistence stays in the app wrapper.

use super::ports::MeshEventSink;
use super::{broadcast_announce, discovery, MeshState};

/// Runtime side-effects for a mesh enabled/disabled transition.
pub fn apply_mesh_enabled<F>(
    mesh: &MeshState,
    enabled: bool,
    sink: &dyn MeshEventSink,
    schedule_announce: F,
) where
    F: FnOnce(MeshState),
{
    apply_mesh_enabled_with(mesh, enabled, sink, resume_mesh, schedule_announce);
}

fn resume_mesh(mesh: &MeshState) {
    if let Err(e) = discovery::start_listeners(mesh.clone()) {
        log::error!("[mesh] resume listeners failed: {}", e);
    }
    discovery::detect_tailscale(mesh);
    discovery::detect_local_ip(mesh);
    discovery::send_immediate_beacon(mesh);
}

fn apply_mesh_enabled_with<R, F>(
    mesh: &MeshState,
    enabled: bool,
    sink: &dyn MeshEventSink,
    resume: R,
    schedule_announce: F,
) where
    R: FnOnce(&MeshState),
    F: FnOnce(MeshState),
{
    if enabled {
        log::info!("[mesh] enabled — resuming announce/advertise");
        resume(mesh);
        schedule_announce(mesh.clone());
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

pub async fn announce_enabled_mesh(mesh: MeshState) {
    let _ = broadcast_announce(&mesh, None).await;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::MeshManager;

    struct RecordingSink;

    impl MeshEventSink for RecordingSink {
        fn emit_mesh_state_changed(&self) {}

        fn emit_config_changed(&self, _: serde_json::Value) {}
    }

    #[test]
    fn enabling_mesh_is_safe_without_an_ambient_tokio_runtime() {
        let scheduled = Arc::new(AtomicBool::new(false));
        let scheduled_on_thread = scheduled.clone();
        let result = std::thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            let mut manager = MeshManager::new("toggle-off-runtime".into(), 0);
            manager.disk_persist = false;
            let mesh = Arc::new(Mutex::new(manager));
            apply_mesh_enabled_with(
                &mesh,
                true,
                &RecordingSink,
                |_| {},
                |_| scheduled_on_thread.store(true, Ordering::Release),
            );
        })
        .join();

        assert!(result.is_ok(), "mesh enable must not require Tokio runtime");
        assert!(scheduled.load(Ordering::Acquire));
    }
}
