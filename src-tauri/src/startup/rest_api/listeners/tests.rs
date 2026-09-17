use super::*;

#[test]
fn mesh_reachability_health_phase_and_publication_share_transition_ownership() {
    let control = RestApiControl::new();
    let generation = control.begin_generation();
    let primary = "127.0.0.1:7690".parse().unwrap();
    control.listener_bound(generation, primary);
    control.mark_listening(generation, vec![primary]).unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let transition = control.transition.clone();
    control.listeners.configure(
        None,
        Arc::new(move |ready| {
            assert!(transition.try_lock().is_none());
            if ready.is_empty() {
                entered_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
        }),
    );
    let failing = control.clone();
    let failure = std::thread::spawn(move || failing.record_unanswered_probe());
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let recovering = control.clone();
    let recovery = std::thread::spawn(move || recovering.record_successful_probe());
    assert_eq!(control.status().phase, RestApiPhase::Failed);
    release_tx.send(()).unwrap();
    assert_eq!(failure.join().unwrap(), RestApiDiagnostic::WorkersBlocked);
    recovery.join().unwrap();
    assert_eq!(control.status().phase, RestApiPhase::Listening);
    assert_eq!(control.listeners.ready(), vec![primary]);
}

#[test]
fn mesh_reachability_health_recovery_cannot_resurrect_a_lost_listener() {
    let publication = ListenerPublication::default();
    let primary = "127.0.0.1:7690".parse().unwrap();
    let remote = "100.64.0.1:7690".parse().unwrap();
    publication.publish(vec![primary, remote]);
    publication.suspend();
    assert!(publication.ready().is_empty());
    publication.resume();
    assert_eq!(publication.ready(), vec![primary, remote]);
    publication.suspend();
    publication.remote_lost(remote);
    publication.resume();
    assert_eq!(publication.ready(), vec![primary]);
    publication.suspend();
    publication.publish(Vec::new());
    publication.resume();
    assert!(publication.ready().is_empty());
}

#[test]
fn mesh_reachability_opt_in_requires_named_authenticated_loopback_and_tailnet() {
    let mut config = crate::config::schema::ApiConfig::default();
    assert_eq!(additional_bind(&config, false).unwrap(), None);
    config.mesh_tailnet_host = Some("100.64.10.1".into());
    assert!(additional_bind(&config, true).is_err());
    config.token = Some("test-only".into());
    assert!(additional_bind(&config, false).is_err());
    assert_eq!(
        additional_bind(&config, true)
            .unwrap()
            .unwrap()
            .ip()
            .to_string(),
        "100.64.10.1"
    );
    for host in [
        "0.0.0.0",
        "127.0.0.1",
        "192.168.1.2",
        "example.test",
        "100.128.0.1",
    ] {
        config.mesh_tailnet_host = Some(host.into());
        assert!(additional_bind(&config, true).is_err());
    }
    config.mesh_tailnet_host = Some("100.64.0.1".into());
    config.host = "0.0.0.0".into();
    assert!(additional_bind(&config, true).is_err());
}

#[test]
fn mesh_reachability_configured_is_not_live_and_remote_loss_preserves_local() {
    let state = ListenerPublication::default();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let sink = observed.clone();
    let primary = "127.0.0.1:7690".parse().unwrap();
    let remote = "100.64.10.1:7690".parse().unwrap();
    state.configure(
        Some(remote),
        Arc::new(move |addresses| sink.lock().push(addresses)),
    );
    assert!(state.ready().is_empty());
    state.publish(vec![primary, remote]);
    state.remote_lost(remote);
    assert_eq!(state.ready(), vec![primary]);
    assert_eq!(*observed.lock(), vec![vec![primary, remote], vec![primary]]);
    state.publish(Vec::new());
    assert!(state.ready().is_empty());
}

/// The release is a type-level guarantee, not a timing one: `bind_remote` takes
/// the primary by value and returns it only in the `Ok` arm, so every error path
/// drops it. That is what this asserts — the refusal.
///
/// It deliberately does not re-bind the address to watch the socket close. Two
/// attempts to observe that from inside the test process failed on Windows even
/// with the I/O driver parked and the address held away from sibling tests, and
/// an assertion that cannot be observed reliably reports the runtime's timing
/// rather than the behaviour it names.
#[tokio::test]
async fn mesh_reachability_failed_secondary_bind_refuses_without_keeping_the_primary() {
    let primary = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let result = bind_remote(primary, Some(occupied.local_addr().unwrap())).await;
    assert!(
        result.is_err(),
        "a secondary bind onto an occupied address must refuse"
    );
}

#[tokio::test]
async fn mesh_reachability_both_bound_before_publication() {
    let primary = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let publication = ListenerPublication::default();
    let (primary, remote) = bind_remote(primary, Some("127.0.0.1:0".parse().unwrap()))
        .await
        .unwrap();
    assert!(publication.ready().is_empty());
    assert!(primary.local_addr().unwrap().port() > 0);
    assert!(remote.unwrap().local_addr().unwrap().port() > 0);
}

#[tokio::test]
async fn mesh_reachability_runtime_remote_exit_preserves_primary_future() {
    let state = ListenerPublication::default();
    let primary = "127.0.0.1:7690".parse().unwrap();
    let remote = "100.64.0.1:7690".parse().unwrap();
    state.publish(vec![primary, remote]);
    let (changed_tx, changed_rx) = oneshot::channel();
    let changed_tx = Mutex::new(Some(changed_tx));
    state.configure(
        Some(remote),
        Arc::new(move |addresses| {
            if addresses == vec![primary] {
                if let Some(sender) = changed_tx.lock().take() {
                    let _ = sender.send(());
                }
            }
        }),
    );
    let (finish_tx, finish_rx) = oneshot::channel();
    let running = tokio::spawn(serve_pair(
        async move {
            finish_rx.await.unwrap();
            Ok(())
        },
        async { Ok(()) },
        remote,
        state.clone(),
        None,
    ));
    changed_rx.await.unwrap();
    assert_eq!(state.ready(), vec![primary]);
    assert!(!running.is_finished());
    finish_tx.send(()).unwrap();
    running.await.unwrap().unwrap();
}

#[tokio::test]
async fn mesh_reachability_startup_remote_exit_fails_closed() {
    let state = ListenerPublication::default();
    let result = serve_pair(
        std::future::pending(),
        async { Ok(()) },
        "100.64.0.1:7690".parse().unwrap(),
        state.clone(),
        None,
    )
    .await;
    assert!(result.is_err());
    assert!(state.ready().is_empty());
}

#[tokio::test]
async fn mesh_reachability_two_real_listeners_serve_the_same_health_router() {
    let primary = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (primary, remote) = bind_remote(primary, Some("127.0.0.1:0".parse().unwrap()))
        .await
        .unwrap();
    let primary_address = primary.local_addr().unwrap();
    let remote_address = remote.as_ref().unwrap().local_addr().unwrap();
    let router = axum::Router::new().route("/api/health", axum::routing::get(|| async { "ok" }));
    let (stop_tx, stop_rx) = oneshot::channel();
    let primary = axum::serve(
        primary,
        router
            .clone()
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = stop_rx.await;
    })
    .into_future();
    let publication = ListenerPublication::default();
    let running = tokio::spawn(serve(primary, remote, router, publication.clone()));
    prove_set(primary_address, Some(remote_address))
        .await
        .unwrap();
    assert!(publication.ready().is_empty());
    publication.publish(vec![primary_address, remote_address]);
    assert_eq!(publication.ready().len(), 2);
    stop_tx.send(()).unwrap();
    running.await.unwrap().unwrap();
}
