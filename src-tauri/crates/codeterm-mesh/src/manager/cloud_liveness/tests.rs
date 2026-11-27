use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::*;
use crate::types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION};
use codeterm_util::cloud_endpoint::CloudEndpoint;

fn inbound_cloud_peer(instance_id: &str) -> PairedPeer {
    PairedPeer {
        instance_id: instance_id.into(),
        instance_name: instance_id.into(),
        addresses: Vec::new(),
        api_port: 0,
        public_key: "pub".into(),
        paired_at: "now".into(),
        last_seen: None,
        online: false,
        is_primary: false,
        access_level: AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[test]
fn pure_model_evaluates_stream_relay_detach_and_admission_gates() {
    let base = InboundCloudEvidence {
        traffic_open: true,
        auth_refused: false,
        installation_unavailable: false,
        stream_attached: true,
        has_cloud_endpoint: true,
        last_relay_ok_at: None,
        now_secs: 1_000,
    };
    assert_eq!(
        evaluate_inbound_cloud_liveness(base),
        InboundCloudLiveness::Online
    );

    // Detach with endpoint still known degrades to Reconnecting.
    assert_eq!(
        evaluate_inbound_cloud_liveness(InboundCloudEvidence {
            stream_attached: false,
            ..base
        }),
        InboundCloudLiveness::Reconnecting
    );

    // Detach without any cloud endpoint degrades to Offline.
    assert_eq!(
        evaluate_inbound_cloud_liveness(InboundCloudEvidence {
            stream_attached: false,
            has_cloud_endpoint: false,
            ..base
        }),
        InboundCloudLiveness::Offline
    );

    // Fresh relay evidence keeps the peer online within TTL, then degrades to Reconnecting.
    assert_eq!(
        evaluate_inbound_cloud_liveness(InboundCloudEvidence {
            stream_attached: false,
            last_relay_ok_at: Some(1_000 - CLOUD_RELAY_EVIDENCE_TTL_SECS),
            ..base
        }),
        InboundCloudLiveness::Online
    );
    assert_eq!(
        evaluate_inbound_cloud_liveness(InboundCloudEvidence {
            stream_attached: false,
            last_relay_ok_at: Some(1_000 - CLOUD_RELAY_EVIDENCE_TTL_SECS - 1),
            ..base
        }),
        InboundCloudLiveness::Reconnecting
    );

    // Admission/auth gates always force Offline even if a stream is attached.
    for blocked in [
        InboundCloudEvidence {
            traffic_open: false,
            ..base
        },
        InboundCloudEvidence {
            auth_refused: true,
            ..base
        },
        InboundCloudEvidence {
            installation_unavailable: true,
            ..base
        },
    ] {
        assert_eq!(
            evaluate_inbound_cloud_liveness(blocked),
            InboundCloudLiveness::Offline
        );
    }
}

#[tokio::test]
async fn inbound_cloud_peer_liveness_tracks_stream_and_relay_and_ignores_address_heartbeats() {
    let clock = Arc::new(AtomicU64::new(500));
    let clock_read = Arc::clone(&clock);
    let mut manager = MeshManager::in_memory(
        crate::types::MeshIdentity {
            instance_id: "home-instance".into(),
            private_key: Some("priv".into()),
            public_key: Some("home-pub".into()),
        },
        "Home".into(),
        7685,
    );
    manager.set_clock(Arc::new(move || clock_read.load(Ordering::SeqCst)));

    let peer_id = "3efa6b47-cbad-4243-b381-86e57930319a";
    let inst_id = "e45c9e4ff42143f0bee45f1f75b11a9c";
    manager.insert_account_peer_with_installation_for_test(
        inst_id.into(),
        inbound_cloud_peer(peer_id),
    );
    manager.insert_cloud_endpoint_for_test(
        inst_id.into(),
        CloudEndpoint::ProviderProxy {
            host: "domios-quill.fly.dev".into(),
        },
    );

    assert!(manager.is_inbound_only_cloud_peer(peer_id));
    assert!(!manager.peers_response().paired[0].online);

    // Stream attach makes peer online in peers_response, account_mesh_pairs, and health check.
    let binding = [7u8; 32];
    manager.attach_reverse_stream(inst_id.into(), binding);
    assert!(manager.peers_response().paired[0].online);
    assert!(manager.account_mesh_pairs()[0].online);
    assert!(manager.has_healthy_connection(inst_id));
    assert_eq!(manager.connection_phases()[0].phase, ConnectionPhase::Ready);

    // Address heartbeat probe skips inbound-only cloud peer and never marks it offline.
    let mesh = Arc::new(parking_lot::Mutex::new(manager));
    let probe = crate::health::check_peer_with(
        &mesh,
        peer_id,
        None,
        |_| async { panic!("address heartbeat must not run for inbound-only cloud peer") },
        |(): (), _| async { panic!("sync must not run for inbound-only cloud peer") },
    )
    .await;
    assert_eq!(probe, None);
    assert!(mesh.lock().peers_response().paired[0].online);

    // Stream detach degrades peer immediately to offline/reconnecting even if relay succeeded earlier.
    mesh.lock().record_cloud_relay_outcome(inst_id, true);
    mesh.lock().detach_reverse_stream(inst_id, &binding);
    {
        let m = mesh.lock();
        assert!(!m.peers_response().paired[0].online);
        assert!(!m.account_mesh_pairs()[0].online);
        assert_eq!(
            m.inbound_cloud_liveness(peer_id),
            Some(InboundCloudLiveness::Reconnecting)
        );
        assert_eq!(m.connection_phases()[0].phase, ConnectionPhase::Retrying);
    }

    // Verified relay success brings it online without a stream until TTL expires or relay fails.
    assert!(mesh.lock().record_cloud_relay_outcome(inst_id, true));
    assert!(mesh.lock().peers_response().paired[0].online);
    clock.store(500 + CLOUD_RELAY_EVIDENCE_TTL_SECS + 1, Ordering::SeqCst);
    assert!(!mesh.lock().peers_response().paired[0].online);

    assert!(mesh.lock().record_cloud_relay_outcome(inst_id, true));
    assert!(mesh.lock().peers_response().paired[0].online);
    assert!(mesh.lock().record_cloud_relay_outcome(inst_id, false));
    assert!(!mesh.lock().peers_response().paired[0].online);
}
