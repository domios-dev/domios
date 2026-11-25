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
    discovery::detect_tailscale(mesh);
    discovery::detect_local_ip(mesh);
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
        log::info!("[mesh] enabled — resuming announce to paired peers");
        resume(mesh);
        mesh.lock().note_mesh_enabled();
        schedule_announce(mesh.clone());
    } else {
        log::info!("[mesh] disabled — going offline, marking peers offline");
        let mut m = mesh.lock();
        for peer in m.paired_peers.values_mut() {
            peer.online = false;
        }
    }

    mesh.lock().notify_connection_change();
    sink.emit_mesh_state_changed();
}

pub async fn announce_enabled_mesh(mesh: MeshState) {
    let online = mesh.lock().build_announce().online;
    if online {
        let _ = broadcast_announce(&mesh, None).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::MeshManager;

    struct RecordingSink {
        mesh_changes: AtomicUsize,
    }

    impl RecordingSink {
        fn new() -> Self {
            Self {
                mesh_changes: AtomicUsize::new(0),
            }
        }
    }

    impl MeshEventSink for RecordingSink {
        fn emit_mesh_state_changed(&self) {
            self.mesh_changes.fetch_add(1, Ordering::AcqRel);
        }

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
            let sink = RecordingSink::new();
            apply_mesh_enabled_with(
                &mesh,
                true,
                &sink,
                |_| {},
                |_| scheduled_on_thread.store(true, Ordering::Release),
            );
            assert_eq!(mesh.lock().mesh_enable_generation(), 1);
            assert_eq!(sink.mesh_changes.load(Ordering::Acquire), 1);
        })
        .join();

        assert!(result.is_ok(), "mesh enable must not require Tokio runtime");
        assert!(scheduled.load(Ordering::Acquire));
    }

    #[test]
    fn off_then_on_marks_peers_offline_notifies_gate_and_bumps_enable_generation() {
        let mut manager = MeshManager::new("toggle-cycle".into(), 0);
        manager.disk_persist = false;
        let peer_id = "peer-1".to_string();
        manager.paired_peers.insert(
            peer_id.clone(),
            crate::types::PairedPeer {
                instance_id: peer_id.clone(),
                instance_name: "Peer".into(),
                addresses: vec![crate::types::PeerAddress::from_ip("100.64.0.2")],
                api_port: 7685,
                public_key: "key".into(),
                paired_at: "2026-01-01T00:00:00Z".into(),
                last_seen: None,
                online: true,
                is_primary: false,
                access_level: crate::types::AccessLevel::Full,
                domios_version: None,
                domios_build: None,
                web_ui: None,
                relay_protocol_version: crate::types::RELAY_PROTOCOL_VERSION,
                consecutive_misses: 0,
                last_active_ms: None,
            },
        );
        let mesh = Arc::new(Mutex::new(manager));
        let mut gate = mesh.lock().gate_watch();
        let sink = RecordingSink::new();

        apply_mesh_enabled_with(&mesh, false, &sink, |_| {}, |_| panic!("must not announce"));
        assert!(!mesh.lock().paired_peers[&peer_id].online);
        assert_eq!(mesh.lock().mesh_enable_generation(), 0);
        assert!(gate.has_changed().unwrap());
        gate.borrow_and_update();

        let scheduled = AtomicBool::new(false);
        apply_mesh_enabled_with(
            &mesh,
            true,
            &sink,
            |_| {},
            |_| {
                scheduled.store(true, Ordering::Release);
            },
        );
        assert!(scheduled.load(Ordering::Acquire));
        assert_eq!(mesh.lock().mesh_enable_generation(), 1);
        assert!(gate.has_changed().unwrap());
        assert_eq!(sink.mesh_changes.load(Ordering::Acquire), 2);
    }
}
