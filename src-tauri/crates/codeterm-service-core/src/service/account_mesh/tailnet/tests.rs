use super::*;
use crate::startup::rest_api::listeners::ListenerPublication;
use codeterm_account::account_mesh::wire::MeshEndpoint;

#[tokio::test]
async fn tailnet_server_exit_withdraws_readiness_and_waits_for_an_external_event() {
    let address = "100.64.1.2:7685".parse().unwrap();
    let mut serving = Serving {
        tasks: JoinSet::new(),
        sockets: BTreeMap::new(),
        failed: BTreeSet::new(),
    };
    let task = serving.tasks.spawn(std::future::pending());
    serving.sockets.insert(address, task);
    serving.sockets[&address].abort();
    let result = serving.tasks.join_next().await.unwrap();
    serving.stopped(result);
    assert!(serving.sockets.is_empty());
    assert!(serving.failed.contains(&address));
}

fn interfaces(values: &[&str]) -> Vec<IpAddr> {
    values.iter().map(|value| value.parse().unwrap()).collect()
}

#[test]
fn tailnet_selection_requires_sign_in_mesh_and_an_assigned_tailnet_address() {
    let assigned = interfaces(&[
        "0.0.0.0",
        "::",
        "127.0.0.1",
        "192.168.1.2",
        "10.1.2.3",
        "100.63.255.255",
        "100.128.0.0",
        "fd7a:115c:a1e1::1",
        "100.64.1.2",
        "fd7a:115c:a1e0::1",
        "100.64.1.2",
    ]);
    for signed_in in [false, true] {
        for enabled in [false, true] {
            let selected = selected_addresses(signed_in, enabled, 7685, assigned.clone());
            assert_eq!(
                selected,
                if signed_in && enabled {
                    vec![
                        "100.64.1.2:7685".parse().unwrap(),
                        "[fd7a:115c:a1e0::1]:7685".parse().unwrap(),
                    ]
                } else {
                    vec![]
                }
            );
        }
    }
    assert!(selected_addresses(true, true, 0, assigned).is_empty());
}

#[test]
fn tailnet_selection_replaces_changed_addresses_and_withdraws_disappeared_addresses() {
    assert_eq!(
        selected_addresses(true, true, 7685, interfaces(&["100.64.1.2"])),
        vec!["100.64.1.2:7685".parse().unwrap()]
    );
    assert_eq!(
        selected_addresses(true, true, 7685, interfaces(&["100.65.1.3"])),
        vec!["100.65.1.3:7685".parse().unwrap()]
    );
    assert!(selected_addresses(true, true, 7685, interfaces(&["192.168.1.2"])).is_empty());
}

#[test]
fn tailnet_publication_integrates_selected_listeners_with_account_endpoint() {
    let manager = codeterm_mesh::MeshManager::in_memory(
        codeterm_mesh::types::MeshIdentity {
            instance_id: "fresh".into(),
            private_key: None,
            public_key: None,
        },
        "fresh".into(),
        7685,
    );
    let mesh = Arc::new(parking_lot::Mutex::new(manager));
    let publication = ListenerPublication::default();
    let observer = mesh.clone();
    publication.configure(
        None,
        Arc::new(move |ready| {
            observer.lock().set_ready_listeners(ready);
        }),
    );
    publication.publish(vec!["127.0.0.1:7685".parse().unwrap()]);
    let endpoint = || {
        mesh.lock()
            .ready_tailnet_endpoint()
            .and_then(|(ip, port)| MeshEndpoint::new(ip, port))
    };
    assert!(endpoint().is_none());
    for ip in ["100.64.1.2", "fd7a:115c:a1e0::1"] {
        publication.publish_tailnet(selected_addresses(true, true, 7685, interfaces(&[ip])));
        assert_eq!(endpoint(), MeshEndpoint::new(ip.parse().unwrap(), 7685));
        publication.publish(vec!["127.0.0.1:7685".parse().unwrap()]);
        assert!(
            endpoint().is_some(),
            "primary readiness must preserve the tailnet listener"
        );
    }
    publication.publish_tailnet(selected_addresses(true, true, 7685, interfaces(&[])));
    assert!(endpoint().is_none());
    publication.publish_tailnet(selected_addresses(
        false,
        true,
        7685,
        interfaces(&["100.64.1.2"]),
    ));
    assert!(endpoint().is_none());
}

#[tokio::test]
async fn tailnet_listener_marks_health_as_peer_only_for_web_publication() {
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;
    let router = Router::new()
        .route(
            "/api/health",
            get(|_: axum::Extension<PeerListener>| async { "healthy" }),
        )
        .layer(axum::middleware::from_fn(mark_peer_listener));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response.headers()[codeterm_mesh::reachability::PEER_ONLY_HEADER],
        "1"
    );
}

#[tokio::test]
async fn tailnet_config_watch_accepts_the_server_generic_config_event() {
    let events = codeterm_events::EventBroadcaster::new();
    let mut receiver = events.subscribe();
    events.publish_generic("unrelated", serde_json::Value::Null);
    events.publish_generic("config-changed", serde_json::Value::Null);
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        config_changed(&mut receiver),
    )
    .await
    .expect("server config event must wake listener selection")
    .unwrap();
}
