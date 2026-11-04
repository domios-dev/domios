use super::*;
use parking_lot::Mutex;
use std::sync::Arc;

const TAILNET_V4: &str = concat!("100.", "64.0.7");
const NEXT_TAILNET_V4: &str = concat!("100.", "64.0.8");
const TAILNET_V6: &str = concat!("fd7a:", "115c:a1e0::7");
const MAGIC_DNS_URL: &str = concat!("https://host.fixture.ts", ".net");

fn make_mesh() -> MeshState {
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

fn status(addresses: &[&str]) -> TransportStatus {
    TransportStatus {
        display_name: "Network".into(),
        state: network_transport::TransportState::Connected,
        identity: None,
        network_name: None,
        user: None,
        addresses: addresses
            .iter()
            .map(|value| value.parse().unwrap())
            .collect(),
        peer_listener: true,
        encrypted: true,
        setup_actions: vec![],
    }
}

#[test]
fn equal_address_sets_and_probe_flaps_never_schedule_another_announce() {
    let mesh = make_mesh();
    let ips = [TAILNET_V4, TAILNET_V6];
    assert!(refresh_with(&mesh, false, |input| Observation::collect(
        input,
        status(&ips),
        Some("192.168.1.7".into()),
        Some(MAGIC_DNS_URL),
        |_| true,
    )));
    let verified = mesh.lock().local_web_ui.clone();
    for addresses in [
        vec![TAILNET_V6, TAILNET_V4, TAILNET_V4],
        vec![TAILNET_V4, "fe80::1", "::1", TAILNET_V6],
    ] {
        assert!(!refresh_with(&mesh, false, |input| Observation::collect(
            input,
            status(&addresses),
            Some("192.168.1.7".into()),
            Some(MAGIC_DNS_URL),
            |_| panic!("unchanged proven endpoint must not be reprobed"),
        )));
        assert_eq!(mesh.lock().local_web_ui, verified);
    }
}

#[test]
fn real_address_change_revalidates_only_new_endpoint_and_retires_old_proof() {
    let mesh = make_mesh();
    refresh_with(&mesh, false, |input| {
        Observation::collect(input, status(&[TAILNET_V4]), None, None, |_| true)
    });
    let mut verified = 0;
    assert!(refresh_with(&mesh, false, |input| Observation::collect(
        input,
        status(&[NEXT_TAILNET_V4]),
        None,
        None,
        |_| {
            verified += 1;
            false
        },
    )));
    assert_eq!(verified, 1);
    assert!(mesh.lock().local_web_ui.is_none());
    assert_eq!(mesh.lock().tailscale_ip.as_deref(), Some(NEXT_TAILNET_V4));
}

#[test]
fn ready_listener_order_and_duplicates_do_not_change_the_refresh_identity() {
    let mesh = make_mesh();
    let listeners = [
        concat!("100.", "64.0.7:7685").parse().unwrap(),
        "127.0.0.1:7685".parse().unwrap(),
    ];
    mesh.lock().ready_listeners = listeners.to_vec();
    assert!(refresh_with(&mesh, false, |input| Observation::collect(
        input,
        status(&[TAILNET_V4]),
        None,
        None,
        |_| true,
    )));
    mesh.lock().ready_listeners = vec![listeners[1], listeners[0], listeners[0]];
    assert!(!refresh_with(&mesh, false, |input| Observation::collect(
        input,
        status(&[TAILNET_V4]),
        None,
        None,
        |_| panic!("already verified"),
    )));
}

#[test]
fn build_and_health_changes_cannot_reuse_old_endpoint_proof() {
    let mesh = make_mesh();
    refresh_with(&mesh, false, |input| {
        Observation::collect(input, status(&[TAILNET_V4]), None, None, |_| true)
    });
    mesh.lock().build_hash = "another-build".into();
    let mut probes = 0;
    assert!(refresh_with(&mesh, false, |input| Observation::collect(
        input,
        status(&[TAILNET_V4]),
        None,
        None,
        |_| {
            probes += 1;
            false
        },
    )));
    assert_eq!(probes, 1);
    assert!(mesh.lock().local_web_ui.is_none());
    mesh.lock().local_http_healthy = false;
    assert!(!refresh_with(&mesh, false, |input| Observation::collect(
        input,
        status(&[TAILNET_V4]),
        None,
        None,
        |_| panic!("unhealthy listener"),
    )));
    assert!(!mesh.lock().build_announce().online);
}

#[tokio::test]
async fn pending_health_probe_never_holds_manager_or_serializes_other_instances() {
    let mesh = make_mesh();
    let first = mesh.clone();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let task = tokio::task::spawn_blocking(move || {
        let mut started = Some(started);
        refresh_with(&first, false, |input| {
            if let Some(started) = started.take() {
                started.send(()).unwrap();
            }
            blocked
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            Observation::collect(input, status(&[TAILNET_V4]), None, None, |_| true)
        })
    });
    bounded(ready).await.unwrap();
    let manager_available = mesh.try_lock().is_some();
    let same_instance_skips =
        manager_available && !refresh_with(&mesh, false, |_| panic!("single flight"));
    let second = make_mesh();
    let other_instance_refreshes = refresh_with(&second, false, |input| {
        Observation::collect(input, status(&[NEXT_TAILNET_V4]), None, None, |_| true)
    });
    release.send(()).unwrap();
    assert!(bounded(task).await.unwrap());
    assert!(manager_available);
    assert!(same_instance_skips);
    assert!(other_instance_refreshes);
}

#[test]
fn forced_refresh_during_probe_is_coalesced_and_applies_latest_configuration() {
    let mesh = make_mesh();
    let mut probes = 0;
    assert!(refresh_with(&mesh, false, |input| {
        probes += 1;
        if probes == 1 {
            assert!(!refresh_with(&mesh, true, |_| panic!(
                "forced refresh must queue"
            )));
            assert!(!refresh_with(&mesh, true, |_| panic!("latest wins")));
            Observation::collect(input, status(&[TAILNET_V4]), None, None, |_| true)
        } else {
            Observation::collect(input, status(&[NEXT_TAILNET_V4]), None, None, |_| true)
        }
    }));
    assert_eq!(probes, 2);
    assert_eq!(mesh.lock().tailscale_ip.as_deref(), Some(NEXT_TAILNET_V4));
}

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(5), future)
        .await
        .expect("fixture event must complete")
}
