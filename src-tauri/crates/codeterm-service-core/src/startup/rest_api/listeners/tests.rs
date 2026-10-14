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
fn mesh_reachability_opt_in_requires_dev_build_authenticated_loopback_and_tailnet() {
    let mut config = crate::config::schema::ApiConfig::default();
    assert_eq!(additional_bind(&config, false).unwrap(), None);
    config.mesh_tailnet_host = Some("100.84.159.37".into());
    assert!(additional_bind(&config, true).is_err());
    config.token = Some("test-only".into());
    assert!(additional_bind(&config, false).is_err());
    assert_eq!(
        additional_bind(&config, true)
            .unwrap()
            .unwrap()
            .ip()
            .to_string(),
        "100.84.159.37"
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
    let remote = "100.84.159.37:7690".parse().unwrap();
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
    let primary = bind_listener("127.0.0.1:0").await.unwrap();
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let result = bind_remote(primary, Some(occupied.local_addr().unwrap())).await;
    assert!(
        result.is_err(),
        "a secondary bind onto an occupied address must refuse"
    );
}

#[tokio::test]
async fn mesh_reachability_both_bound_before_publication() {
    let primary = bind_listener("127.0.0.1:0").await.unwrap();
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
async fn mesh_tailnet_two_listeners_are_health_proven_and_published() {
    let primary = bind_listener("127.0.0.1:0").await.unwrap();
    let (primary, remote) = bind_remote(primary, Some("127.0.0.1:0".parse().unwrap()))
        .await
        .unwrap();
    prove_and_publish_listener_pair(primary, remote).await;
}

#[tokio::test]
async fn mesh_tailnet_listener_registered_on_parked_reactor_cannot_accept() {
    let (binding_runtime, listener) = tokio::task::spawn_blocking(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime.block_on(async {
            let primary = bind_listener("127.0.0.1:0").await.unwrap();
            let (_, remote) = bind_remote(primary, Some("127.0.0.1:0".parse().unwrap()))
                .await
                .unwrap();
            tokio::net::TcpListener::from_std(remote.unwrap()).unwrap()
        });
        (runtime, listener)
    })
    .await
    .unwrap();
    let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let accepted = tokio::time::timeout(Duration::from_millis(250), listener.accept()).await;
    assert!(
        accepted.is_err(),
        "a parked reactor must not drive acceptance"
    );
    drop(client);
    drop(listener);
    binding_runtime.shutdown_background();
}

#[tokio::test]
async fn mesh_tailnet_production_bind_transfers_from_parked_reactor_to_serving_runtime() {
    let primary = bind_listener("127.0.0.1:0").await.unwrap();
    let (binding_runtime, primary, remote) = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (primary, remote) = runtime
            .block_on(bind_remote(primary, Some("127.0.0.1:0".parse().unwrap())))
            .unwrap();
        (runtime, primary, remote)
    })
    .await
    .unwrap();
    prove_and_publish_listener_pair(primary, remote).await;
    tokio::task::spawn_blocking(move || drop(binding_runtime))
        .await
        .unwrap();
}

async fn prove_and_publish_listener_pair(
    primary: std::net::TcpListener,
    remote: Option<std::net::TcpListener>,
) {
    let primary_address = primary.local_addr().unwrap();
    let remote_address = remote.as_ref().unwrap().local_addr().unwrap();
    let router = axum::Router::new().route("/api/health", axum::routing::get(|| async { "ok" }));
    let (stop_tx, stop_rx) = oneshot::channel();
    let primary = serve_listener(primary, router.clone(), stop_rx);
    let control = RestApiControl::new();
    let generation = control.begin_generation();
    control.listener_bound(generation, primary_address);
    let server = serve(primary, remote, router, control.listeners.clone());
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => panic!("listeners exited before proof: {result:?}"),
        result = prove_set(primary_address, Some(remote_address)) => result.unwrap(),
    }
    assert!(control.listeners.ready().is_empty());
    let status = control
        .mark_listening(generation, vec![primary_address, remote_address])
        .unwrap();
    assert_eq!(status.phase, RestApiPhase::Listening);
    assert_eq!(
        control.listeners.ready(),
        vec![primary_address, remote_address]
    );
    let request = async {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{remote_address}/api/health"))
            .header(reqwest::header::CONNECTION, "close")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    };
    tokio::select! {
        result = &mut server => panic!("listeners exited after publication: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(5), request) => {
            assert_eq!(result.unwrap(), "ok");
        },
    }
    stop_tx.send(()).unwrap();
    server.await.unwrap();
    assert!(control.listeners.ready().is_empty());
}

#[tokio::test]
async fn mesh_tailnet_health_proof_deadline_bounds_an_incomplete_response_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (headers_tx, mut headers_rx) = oneshot::channel();
    let stalled_response = async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
            .await
            .unwrap();
        headers_tx.send(()).unwrap();
        std::future::pending::<()>().await;
    };
    tokio::pin!(stalled_response);
    let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
    let error = tokio::select! {
        _ = &mut stalled_response => panic!("stalled response unexpectedly completed"),
        result = prove_health(address, deadline) => result.unwrap_err(),
    };
    headers_rx
        .try_recv()
        .expect("the client must have received response headers");
    assert!(error.contains(&address.to_string()), "{error}");
    assert!(error.contains("failure deadline"), "{error}");
}
