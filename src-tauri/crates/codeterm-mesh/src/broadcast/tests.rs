use super::*;
use parking_lot::Mutex;
use std::sync::Arc;

fn mesh() -> MeshState {
    Arc::new(Mutex::new(crate::MeshManager::in_memory(
        crate::types::MeshIdentity {
            instance_id: "local".into(),
            private_key: None,
            public_key: None,
        },
        "Local".into(),
        7685,
    )))
}

fn work(mesh: &MeshState, id: &str) -> AnnounceWork {
    AnnounceWork {
        addresses: vec![PeerAddress::from_ip(concat!("100.", "64.0.7"))],
        port: 7685,
        token: "opaque".into(),
        peer_id: id.into(),
        announce: mesh.lock().build_announce(),
        membership: None,
    }
}

fn paired_peer() -> crate::types::PairedPeer {
    crate::types::PairedPeer {
        instance_id: "known".into(),
        instance_name: "Known".into(),
        addresses: vec![PeerAddress::from_ip(concat!("100.", "64.0.9"))],
        api_port: 7685,
        public_key: "opaque".into(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: Default::default(),
        version: None,
        web_ui: None,
        relay_protocol_version: crate::types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[test]
fn peer_request_error_keeps_identity_and_drops_urls() {
    let error = peer_request_error("peer-id", "failed http://192.0.2.10:7685/api/mesh/announce");
    assert_eq!(error, "peer-id: request failed");
}

#[tokio::test]
async fn stalled_peer_does_not_serialize_fanout_or_hold_inbound_manager() {
    let mesh = mesh();
    mesh.lock()
        .paired_peers
        .insert("known".into(), paired_peer());
    let works = vec![work(&mesh, "slow"), work(&mesh, "fast")];
    let (started, ready) = tokio::sync::oneshot::channel();
    let (fast, fast_ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = tokio::sync::oneshot::channel();
    let started = Arc::new(Mutex::new(Some(started)));
    let fast = Arc::new(Mutex::new(Some(fast)));
    let blocked = Arc::new(Mutex::new(Some(blocked)));
    let sender: Arc<SendPort> = Arc::new(move |work| {
        let started = started.lock().take();
        let fast = (work.peer_id == "fast")
            .then(|| fast.lock().take())
            .flatten();
        let blocked = (work.peer_id == "slow")
            .then(|| blocked.lock().take())
            .flatten();
        Box::pin(async move {
            if let Some(started) = started {
                started.send(()).unwrap();
            }
            if let Some(fast) = fast {
                fast.send(()).unwrap();
            }
            if let Some(blocked) = blocked {
                bounded(blocked).await.unwrap();
            }
            SendOutcome::Acknowledged
        })
    });
    let running = {
        let mesh = mesh.clone();
        tokio::spawn(async move { fanout(&mesh, works, sender, Duration::from_secs(30)).await })
    };
    bounded(ready).await.unwrap();
    let fast_completed = tokio::time::timeout(Duration::from_secs(1), fast_ready).await;
    let inbound = mesh.try_lock().map(|mut manager| {
        let mut announce = manager.build_announce();
        announce.instance_id = "known".into();
        announce.instance_name = "Updated".into();
        announce.online = false;
        manager.apply_announce("known", &announce).unwrap();
        manager.build_announce().instance_id
    });
    release.send(()).unwrap();
    let report = bounded(running).await.unwrap();
    assert!(fast_completed.unwrap().is_ok());
    assert_eq!(inbound.as_deref(), Some("local"));
    assert_eq!(report.sent, 2);
    assert_eq!(report.ok, 2);
    assert_eq!(mesh.lock().paired_peers["known"].instance_name, "Updated");
}

#[tokio::test]
async fn peer_timeout_is_total_and_releases_send_capacity() {
    let mesh = mesh();
    let works = vec![work(&mesh, "a"), work(&mesh, "b")];
    let sender: Arc<SendPort> = Arc::new(|_| Box::pin(std::future::pending()));
    let report = fanout(&mesh, works, sender, Duration::ZERO).await;
    assert_eq!(report.sent, 2);
    assert_eq!(report.ok, 0);
    assert_eq!(
        report.errors,
        vec!["a: request failed", "b: request failed"]
    );
    let manager = mesh.lock();
    assert!(manager.has_unacked_announce("a"));
    assert!(manager.has_unacked_announce("b"));
    assert_eq!(manager.announce_slots.available_permits(), 8);
}

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(5), future)
        .await
        .expect("fixture event must complete")
}
