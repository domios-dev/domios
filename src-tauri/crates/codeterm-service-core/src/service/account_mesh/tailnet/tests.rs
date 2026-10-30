use super::*;
use crate::startup::rest_api::listeners::ListenerPublication;
use codeterm_account::account_mesh::wire::MeshEndpoint;

#[tokio::test]
async fn tailnet_server_exit_withdraws_readiness_and_waits_for_an_external_event() {
    let address = "100.64.1.2:7685".parse().unwrap();
    let mut serving = Serving {
        tasks: JoinSet::new(),
        sockets: BTreeMap::new(),
        failed: BTreeMap::new(),
        reused: BTreeSet::new(),
    };
    let task = serving.tasks.spawn(std::future::pending());
    serving.sockets.insert(address, task);
    serving.sockets[&address].abort();
    let result = serving.tasks.join_next().await.unwrap();
    serving.stopped(result);
    assert!(serving.sockets.is_empty());
    assert!(serving.failed.contains_key(&address));
}

fn interfaces(values: &[&str]) -> Vec<IpAddr> {
    values.iter().map(|value| value.parse().unwrap()).collect()
}

#[test]
fn tailnet_selection_requires_api_and_an_assigned_tailnet_address() {
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
    for enabled in [false, true] {
        let selected = selected_addresses(enabled, 7685, assigned.clone());
        assert_eq!(
            selected,
            if enabled {
                vec![
                    "100.64.1.2:7685".parse().unwrap(),
                    "[fd7a:115c:a1e0::1]:7685".parse().unwrap(),
                ]
            } else {
                vec![]
            }
        );
    }
    assert!(selected_addresses(true, 0, assigned).is_empty());
}

#[test]
fn tailnet_selection_replaces_changed_addresses_and_withdraws_disappeared_addresses() {
    assert_eq!(
        selected_addresses(true, 7685, interfaces(&["100.64.1.2"])),
        vec!["100.64.1.2:7685".parse().unwrap()]
    );
    assert_eq!(
        selected_addresses(true, 7685, interfaces(&["100.65.1.3"])),
        vec!["100.65.1.3:7685".parse().unwrap()]
    );
    assert!(selected_addresses(true, 7685, interfaces(&["192.168.1.2"])).is_empty());
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
        mesh.lock()
            .set_network_transport(codeterm_mesh::network_transport::TransportStatus {
                display_name: "Test network".into(),
                state: codeterm_mesh::network_transport::TransportState::Connected,
                identity: None,
                network_name: None,
                user: None,
                addresses: interfaces(&[ip]),
                peer_listener: true,
                encrypted: true,
                setup_actions: Vec::new(),
            });
        publication.publish_tailnet(selected_addresses(true, 7685, interfaces(&[ip])));
        assert_eq!(endpoint(), MeshEndpoint::new(ip.parse().unwrap(), 7685));
        publication.publish(vec!["127.0.0.1:7685".parse().unwrap()]);
        assert!(
            endpoint().is_some(),
            "primary readiness must preserve the tailnet listener"
        );
    }
    publication.publish_tailnet(selected_addresses(true, 7685, interfaces(&[])));
    assert!(endpoint().is_none());
}

#[tokio::test]
async fn tailnet_listener_marks_requests_and_allows_web_publication() {
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;
    let router = Router::new()
        .route(
            "/api/health",
            get(|_: axum::Extension<TailnetListener>| async { "healthy" }),
        )
        .layer(axum::middleware::from_fn(mark_tailnet_listener));
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
    assert!(!response
        .headers()
        .contains_key(codeterm_mesh::reachability::PEER_ONLY_HEADER));
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

#[test]
fn tailnet_wildcard_reuses_only_the_covered_family_and_port() {
    let v4 = "100.64.1.2:7685".parse().unwrap();
    let v6 = "[fd7a:115c:a1e0::1]:7685".parse().unwrap();
    assert!(covers("0.0.0.0:7685".parse().unwrap(), v4));
    assert!(covers("[::]:7685".parse().unwrap(), v6));
    assert!(!covers("0.0.0.0:7685".parse().unwrap(), v6));
    assert!(!covers("0.0.0.0:7686".parse().unwrap(), v4));
    assert!(!covers("127.0.0.1:7685".parse().unwrap(), v4));
}

#[test]
fn tailnet_health_failures_back_off_and_log_only_changed_states() {
    let mut failure = retry::Failure::default();
    let mut now = tokio::time::Instant::now();
    for delay in [30, 60, 120, 240, 300, 300] {
        let first = failure.retry_at().is_none();
        assert_eq!(failure.record(now, "health deadline".into()), first);
        assert!(!failure.due(now));
        assert!(!failure.due(now + std::time::Duration::from_secs(delay - 1)));
        now += std::time::Duration::from_secs(delay);
        assert!(failure.due(now));
    }
    assert!(failure.record(now, "address unavailable".into()));
}

#[tokio::test]
async fn tailnet_proven_wildcard_is_ready_without_a_second_bind() {
    let address = "100.64.1.2:7685".parse().unwrap();
    let mut serving = Serving {
        tasks: JoinSet::new(), sockets: BTreeMap::new(),
        failed: BTreeMap::new(), reused: BTreeSet::new(),
    };
    let mut attempts = 0;
    serving.reconcile_with_bind(&[address], &axum::Router::new(), &["0.0.0.0:7685".parse().unwrap()], |_| {
        attempts += 1;
        Err(std::io::Error::from(std::io::ErrorKind::AddrInUse))
    }).await;
    assert_eq!(attempts, 1);
    assert_eq!(serving.ready(), vec![address]);
    assert!(serving.tasks.is_empty());
    assert!(serving.failed.is_empty());
    serving.withdraw(&[]).await;
    assert!(serving.ready().is_empty());
}

#[tokio::test]
async fn tailnet_wildcard_does_not_replace_an_unavailable_dedicated_address() {
    let address = "100.64.1.2:7685".parse().unwrap();
    let mut serving = Serving {
        tasks: JoinSet::new(), sockets: BTreeMap::new(),
        failed: BTreeMap::new(), reused: BTreeSet::new(),
    };
    serving.reconcile_with_bind(&[address], &axum::Router::new(), &["0.0.0.0:7685".parse().unwrap()], |_| {
        Err(std::io::Error::from(std::io::ErrorKind::AddrNotAvailable))
    }).await;
    assert!(serving.ready().is_empty());
    assert!(serving.failed.contains_key(&address));
}

#[test]
fn tailnet_external_event_resets_deadline_and_retains_backoff_and_diagnostic() {
    let address = "100.64.1.2:7685".parse().unwrap();
    let now = tokio::time::Instant::now();
    let mut serving = Serving {
        tasks: JoinSet::new(), sockets: BTreeMap::new(),
        failed: BTreeMap::new(), reused: BTreeSet::new(),
    };
    let failure = serving.failed.entry(address).or_default();
    assert!(failure.record(now, "address unavailable".into()));
    assert!(!failure.due(now));
    serving.retry_now();
    let failure = serving.failed.get_mut(&address).unwrap();
    assert!(failure.due(now));
    assert!(!failure.record(now, "address unavailable".into()));
    assert_eq!(failure.retry_at(), Some(now + std::time::Duration::from_secs(60)));
}
