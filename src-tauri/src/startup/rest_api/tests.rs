use super::*;

const LIFECYCLE_TEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

async fn reserve_initial_listener(control: &RestApiControl) -> SocketAddr {
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_reuseaddr(true).unwrap();
    #[cfg(unix)]
    enable_shared_port_binding(&socket).unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(1024).unwrap();
    let address = listener.local_addr().unwrap();
    control.install_initial_listener(listener);
    address
}

async fn wait_for_status<F>(
    control: &RestApiControl,
    status: &mut watch::Receiver<RestApiStatus>,
    expected: F,
) -> RestApiStatus
where
    F: Fn(&RestApiStatus) -> bool,
{
    let mut last_observed = status.borrow().clone();
    let result = tokio::time::timeout(LIFECYCLE_TEST_DEADLINE, async {
        loop {
            let current = status.borrow_and_update().clone();
            last_observed = current.clone();
            if expected(&current) {
                return current;
            }
            status.changed().await.unwrap();
        }
    })
    .await;
    result
        .unwrap_or_else(|_| panic_lifecycle_deadline("listener lifecycle", &last_observed, control))
}

async fn wait_for_lifecycle<F>(
    control: &RestApiControl,
    events: &mut broadcast::Receiver<RestApiStatus>,
    expected: F,
) -> RestApiStatus
where
    F: Fn(&RestApiStatus) -> bool,
{
    let mut last_observed = control.status();
    let result = tokio::time::timeout(LIFECYCLE_TEST_DEADLINE, async {
        loop {
            let event = events.recv().await.expect("lifecycle sender stopped");
            last_observed = event.clone();
            if expected(&event) {
                return event;
            }
        }
    })
    .await;
    result.unwrap_or_else(|_| panic_lifecycle_deadline("REST lifecycle", &last_observed, control))
}

fn panic_lifecycle_deadline(
    label: &str,
    last_observed: &RestApiStatus,
    control: &RestApiControl,
) -> ! {
    let current = control.status();
    panic!(
        "{label} event deadline elapsed; last observed phase={:?} generation={} diagnostic={:?}; current phase={:?} generation={} diagnostic={:?}; listener_owned={}",
        last_observed.phase,
        last_observed.generation,
        last_observed.diagnostic,
        current.phase,
        current.generation,
        current.diagnostic,
        control.listener_owned()
    )
}

fn health_router() -> axum::Router {
    axum::Router::new().route("/api/health", axum::routing::get(|| async { "ok" }))
}

fn assert_recoverable_failure(status: &RestApiStatus, detail: &str) {
    let message = status
        .message
        .as_deref()
        .expect("failure must explain recovery");
    assert!(message.contains(detail));
    assert!(!message.contains("Restart CodeTerm"));
}

#[tokio::test]
async fn occupied_port_release_event_rebinds_the_real_listener() {
    let control = Arc::new(RestApiControl::new());
    let address = reserve_initial_listener(&control).await;
    control.force_bind_error();
    let mut status = control.subscribe_status();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    let failed = wait_for_status(&control, &mut status, |s| s.phase == RestApiPhase::Failed).await;
    assert!(failed
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Release the port")));
    control.port_released().unwrap();
    wait_for_status(&control, &mut status, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;
    let response = reqwest::get(format!("http://{address}/api/health"))
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    task.abort();
}

#[tokio::test]
async fn manual_restart_rebinds_a_real_listener() {
    let control = Arc::new(RestApiControl::new());
    let address = reserve_initial_listener(&control).await;
    let mut status = control.subscribe_status();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    let mut previous = wait_for_status(&control, &mut status, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;
    for _ in 0..8 {
        let returned = control.restart_and_wait().await.unwrap();
        assert_eq!(
            returned.phase,
            RestApiPhase::Listening,
            "{:?}",
            returned.message
        );
        assert!(returned.generation > previous.generation);
        previous = returned;
    }
    let stream = tokio::net::TcpStream::connect(address).await.unwrap();
    drop(stream);
    task.abort();
}

#[tokio::test]
async fn port_release_while_listening_keeps_the_real_listener_running() {
    let control = Arc::new(RestApiControl::new());
    let address = reserve_initial_listener(&control).await;
    let mut status = control.subscribe_status();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    control.set_port_release_barrier(barrier.clone());
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    let listening = wait_for_status(&control, &mut status, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;

    control.port_released().unwrap();
    barrier.wait().await;

    assert_eq!(control.status(), listening);
    let response = reqwest::get(format!("http://{address}/api/health"))
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    task.abort();
}

#[tokio::test]
async fn unexpected_exit_autonomously_replaces_generation_after_health_proof() {
    let control = Arc::new(RestApiControl::new());
    let exit = control.install_server_exit();
    let mut lifecycle = control.subscribe_lifecycle();
    let mut recovery_events = control.take_recovery_events().unwrap();
    let task = tokio::spawn(start(
        control.clone(),
        "127.0.0.1:0".to_string(),
        health_router(),
    ));
    let initial = wait_for_lifecycle(&control, &mut lifecycle, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;
    let initial_address = control
        .bound_addr()
        .expect("health-proven generation must own its listener");
    eprintln!(
        "REST generation={} address={} bind_succeeded=true diagnostic={:?}",
        initial.generation, initial_address, initial.diagnostic
    );

    exit.send(()).unwrap();
    let failed = wait_for_lifecycle(&control, &mut lifecycle, |s| {
        s.phase == RestApiPhase::Failed
    })
    .await;
    assert_eq!(failed.generation, initial.generation);
    assert_eq!(failed.diagnostic, Some(RestApiDiagnostic::ListenerExited));
    assert_recoverable_failure(&failed, "exited unexpectedly");
    let recovered = wait_for_lifecycle(&control, &mut lifecycle, |s| {
        s.generation > failed.generation && s.phase != RestApiPhase::Starting
    })
    .await;
    let recovered_address = control.bound_addr();
    eprintln!(
        "REST generation={} address={:?} bind_succeeded={} diagnostic={:?}",
        recovered.generation,
        recovered_address,
        recovered_address.is_some(),
        recovered.diagnostic
    );
    assert_eq!(recovered.phase, RestApiPhase::Listening);
    assert_eq!(recovered.generation, failed.generation + 1);
    assert_eq!(recovered.diagnostic, None);
    let recovered_address =
        recovered_address.expect("replacement generation must own its listener");
    let recovery = tokio::time::timeout(std::time::Duration::from_secs(5), recovery_events.recv())
        .await
        .expect("recovery signal deadline elapsed")
        .expect("recovery signal owner stopped");
    assert_eq!(recovery.status.generation, recovered.generation);
    assert_eq!(recovery.transition_id, 1);
    let response = reqwest::get(format!("http://{recovered_address}/api/health"))
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    control.shutdown_and_wait().await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn shutdown_error_fails_its_restart_completion_then_recovers() {
    let control = Arc::new(RestApiControl::new());
    let address = reserve_initial_listener(&control).await;
    control.force_shutdown_error();
    let mut lifecycle = control.subscribe_lifecycle();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    wait_for_lifecycle(&control, &mut lifecycle, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;

    let restart_control = control.clone();
    let restart = tokio::spawn(async move { restart_control.restart_and_wait().await.unwrap() });
    let failed = wait_for_lifecycle(&control, &mut lifecycle, |s| {
        s.phase == RestApiPhase::Failed
    })
    .await;
    let completed = restart.await.unwrap();
    assert_eq!(completed, failed);
    assert_recoverable_failure(&completed, "shutdown failed");
    let recovered = wait_for_lifecycle(&control, &mut lifecycle, |s| {
        s.phase == RestApiPhase::Listening && s.generation > failed.generation
    })
    .await;
    assert_eq!(recovered.phase, RestApiPhase::Listening);
    assert_eq!(recovered.generation, completed.generation + 1);
    let response = reqwest::get(format!("http://{address}/api/health"))
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    control.shutdown_and_wait().await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn overlapping_restarts_complete_from_their_own_listener_generations() {
    let control = Arc::new(RestApiControl::new());
    let address = reserve_initial_listener(&control).await;
    let mut status = control.subscribe_status();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    control.set_restart_barrier(barrier.clone());
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    wait_for_status(&control, &mut status, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;

    let first_control = control.clone();
    let first = tokio::spawn(async move { first_control.restart_and_wait().await.unwrap() });
    let second_control = control.clone();
    let second = tokio::spawn(async move { second_control.restart_and_wait().await.unwrap() });
    barrier.wait().await;

    let first = first.await.unwrap();
    let second = second.await.unwrap();
    assert_eq!(first.phase, RestApiPhase::Listening);
    assert_eq!(second.phase, RestApiPhase::Listening);
    assert!(second.generation > first.generation);
    task.abort();
}

#[test]
fn status_read_reports_generation_phase_and_failure_message() {
    let control = RestApiControl::new();
    let generation = control.begin_generation();
    control.fail_generation(
        generation,
        RestApiDiagnostic::BindOwnershipFailed,
        "port is occupied".into(),
        false,
    );
    assert_eq!(
        control.status(),
        RestApiStatus {
            generation: 1,
            phase: RestApiPhase::Failed,
            diagnostic: Some(RestApiDiagnostic::BindOwnershipFailed),
            message: Some("port is occupied".into()),
        }
    );
}

struct WorkerGates {
    capacity: tokio::sync::Semaphore,
    entered: tokio::sync::Barrier,
    release: tokio::sync::Barrier,
    health_requests: mpsc::UnboundedSender<()>,
}

fn send_request(address: SocketAddr, path: &'static str) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}{path}"))
            .header(reqwest::header::CONNECTION, "close")
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        response.bytes().await.unwrap();
    })
}

fn worker_gated_router(gates: Arc<WorkerGates>) -> axum::Router {
    let health_gates = gates.clone();
    axum::Router::new()
        .route(
            "/api/health",
            axum::routing::get(move || {
                let gates = health_gates.clone();
                async move {
                    let _ = gates.health_requests.send(());
                    let _capacity = gates.capacity.acquire().await.unwrap();
                    "ok"
                }
            }),
        )
        .route(
            "/stall",
            axum::routing::get(move || {
                let gates = gates.clone();
                async move {
                    let _capacity = gates.capacity.acquire().await.unwrap();
                    gates.entered.wait().await;
                    gates.release.wait().await;
                    "released"
                }
            }),
        )
}

#[tokio::test]
async fn diagnostics_distinguish_listener_exit_from_worker_stall() {
    let exit_control = Arc::new(RestApiControl::new());
    let exit_address = reserve_initial_listener(&exit_control).await;
    let exit = exit_control.install_server_exit();
    let mut exit_events = exit_control.subscribe_lifecycle();
    let exit_task = tokio::spawn(start(
        exit_control.clone(),
        exit_address.to_string(),
        health_router(),
    ));
    wait_for_lifecycle(&exit_control, &mut exit_events, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;
    exit.send(()).unwrap();
    let listener_failure = wait_for_lifecycle(&exit_control, &mut exit_events, |s| {
        s.phase == RestApiPhase::Failed
    })
    .await;
    assert_eq!(
        listener_failure.diagnostic,
        Some(RestApiDiagnostic::ListenerExited)
    );
    exit_control.shutdown_and_wait().await.unwrap();
    exit_task.await.unwrap();

    let stall_control = Arc::new(RestApiControl::new());
    let stall_address = reserve_initial_listener(&stall_control).await;
    let mut stall_events = stall_control.subscribe_lifecycle();
    let (health_requests, mut health_events) = mpsc::unbounded_channel();
    let gates = Arc::new(WorkerGates {
        capacity: tokio::sync::Semaphore::new(4),
        entered: tokio::sync::Barrier::new(5),
        release: tokio::sync::Barrier::new(5),
        health_requests,
    });
    let router = worker_gated_router(gates.clone());
    let server_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let server_runtime_handle = server_runtime.handle().clone();
    let server_control = stall_control.clone();
    let server = server_runtime.spawn(start(server_control, stall_address.to_string(), router));
    let listening = wait_for_lifecycle(&stall_control, &mut stall_events, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;
    health_events
        .recv()
        .await
        .expect("startup health proof must reach the worker gate");
    let clients = (0..4)
        .map(|_| send_request(stall_address, "/stall"))
        .collect::<Vec<_>>();
    gates.entered.wait().await;
    let (runtime_probe, runtime_observed) = oneshot::channel();
    server_runtime_handle.spawn(async move {
        let _ = runtime_probe.send(());
    });
    tokio::time::timeout(LIFECYCLE_TEST_DEADLINE, runtime_observed)
        .await
        .expect("server runtime starved while REST worker capacity was exhausted")
        .expect("server runtime probe stopped");
    let health = send_request(stall_address, "/api/health");
    health_events
        .recv()
        .await
        .expect("health probe must reach the exhausted worker gate");
    assert!(
        !health.is_finished(),
        "health probe must wait while worker capacity is exhausted"
    );

    assert_eq!(
        stall_control.record_unanswered_probe(),
        RestApiDiagnostic::WorkersBlocked
    );
    let worker_failure = wait_for_lifecycle(&stall_control, &mut stall_events, |s| {
        s.phase == RestApiPhase::Failed
    })
    .await;
    assert_eq!(worker_failure.generation, listening.generation);
    assert_eq!(
        worker_failure.diagnostic,
        Some(RestApiDiagnostic::WorkersBlocked)
    );
    assert_ne!(worker_failure.diagnostic, listener_failure.diagnostic);
    assert_ne!(stall_control.status().phase, RestApiPhase::Listening);

    gates.release.wait().await;
    for client in clients {
        client.await.unwrap();
    }
    health.await.unwrap();
    stall_control.record_successful_probe();
    assert_eq!(stall_control.status().phase, RestApiPhase::Listening);
    assert_eq!(stall_control.status().generation, listening.generation);

    stall_control.shutdown_and_wait().await.unwrap();
    server.await.unwrap();
    server_runtime.shutdown_background();
}

#[tokio::test]
async fn repeated_same_generation_outages_emit_exactly_one_resync_each() {
    let control = RestApiControl::new();
    let mut recovery_events = control.take_recovery_events().unwrap();
    let generation = control.begin_generation();
    control.listener_bound(generation, "127.0.0.1:7686".parse().unwrap());
    control.mark_listening(generation).unwrap();
    let mut claims = crate::liveness::RecoveryResyncClaims::default();

    for expected_transition in 1..=2 {
        assert_eq!(
            control.record_unanswered_probe(),
            RestApiDiagnostic::WorkersBlocked
        );
        control.record_successful_probe();
        let recovery = recovery_events
            .recv()
            .await
            .expect("each outage recovery must emit an event");
        assert_eq!(recovery.status.generation, generation);
        assert_eq!(recovery.transition_id, expected_transition);
        assert!(claims.claim(recovery.transition_id));
        assert!(
            !claims.claim(recovery.transition_id),
            "duplicate observation must not claim another resync"
        );
        assert!(
            recovery_events.try_recv().is_err(),
            "one recovery transition must enqueue exactly one event"
        );
    }
}

#[tokio::test]
async fn api_host_change_reports_active_listener() {
    let control = Arc::new(RestApiControl::new());
    let address = reserve_initial_listener(&control).await;
    let mut status = control.subscribe_status();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    wait_for_status(&control, &mut status, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;

    assert_eq!(control.bound_addr(), Some(address));
    assert_eq!(
        control.pending_bind(&address.to_string()),
        None,
        "a config value the live socket already serves is applied, not pending"
    );

    let diverged = format!("127.0.0.1:{}", address.port() + 1);
    let pending = control
        .pending_bind(&diverged)
        .expect("a configured bind the live socket does not serve must be disclosed as pending");
    assert_eq!(pending.live, address.to_string());
    assert_eq!(pending.configured, diverged);
    task.abort();
}

#[tokio::test]
async fn unbindable_configured_value_is_disclosed_not_assumed_applied() {
    let control = Arc::new(RestApiControl::new());
    let address = reserve_initial_listener(&control).await;
    let mut status = control.subscribe_status();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    wait_for_status(&control, &mut status, |s| {
        s.phase == RestApiPhase::Listening
    })
    .await;

    let pending = control
        .pending_bind(&format!("not-an-address:{}", address.port()))
        .expect("a configured value that cannot be proven live must never read back as applied");
    assert_eq!(pending.live, address.to_string());
    task.abort();
}

#[tokio::test]
async fn pending_bind_is_silent_without_a_bound_listener() {
    let control = RestApiControl::new();
    assert_eq!(control.bound_addr(), None);
    assert_eq!(
        control.pending_bind("127.0.0.1:7686"),
        None,
        "with no listener owned there is no live bind to contradict"
    );
}
