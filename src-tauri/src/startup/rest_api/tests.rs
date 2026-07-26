use super::*;
use std::io::{Read, Write};

static REAL_SOCKET_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const LIFECYCLE_TEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

async fn wait_for_status<F>(
    status: &mut watch::Receiver<RestApiStatus>,
    expected: F,
) -> RestApiStatus
where
    F: Fn(&RestApiStatus) -> bool,
{
    tokio::time::timeout(LIFECYCLE_TEST_DEADLINE, async {
        loop {
            let current = status.borrow_and_update().clone();
            if expected(&current) {
                return current;
            }
            status.changed().await.unwrap();
        }
    })
    .await
    .expect("listener lifecycle event deadline elapsed")
}

async fn wait_for_lifecycle<F>(
    events: &mut broadcast::Receiver<RestApiStatus>,
    expected: F,
) -> RestApiStatus
where
    F: Fn(&RestApiStatus) -> bool,
{
    tokio::time::timeout(LIFECYCLE_TEST_DEADLINE, async {
        loop {
            let event = events.recv().await.expect("lifecycle sender stopped");
            if expected(&event) {
                return event;
            }
        }
    })
    .await
    .expect("REST lifecycle event deadline elapsed")
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
    let _socket_owner = REAL_SOCKET_TEST.lock().await;
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = occupied.local_addr().unwrap();
    let control = Arc::new(RestApiControl::new());
    let mut status = control.subscribe_status();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    let failed = wait_for_status(&mut status, |s| s.phase == RestApiPhase::Failed).await;
    assert!(failed
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Release the port")));
    drop(occupied);
    control.port_released().unwrap();
    wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;
    let response = reqwest::get(format!("http://{address}/api/health"))
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    task.abort();
}

#[tokio::test]
async fn manual_restart_rebinds_a_real_listener() {
    let _socket_owner = REAL_SOCKET_TEST.lock().await;
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let control = Arc::new(RestApiControl::new());
    let mut status = control.subscribe_status();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    let mut previous = wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;
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
    let _socket_owner = REAL_SOCKET_TEST.lock().await;
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let control = Arc::new(RestApiControl::new());
    let mut status = control.subscribe_status();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    control.set_port_release_barrier(barrier.clone());
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    let listening = wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;

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
    let _socket_owner = REAL_SOCKET_TEST.lock().await;
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let control = Arc::new(RestApiControl::new());
    let exit = control.install_server_exit();
    let mut lifecycle = control.subscribe_lifecycle();
    let mut recovery_events = control.take_recovery_events().unwrap();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    let initial = wait_for_lifecycle(&mut lifecycle, |s| s.phase == RestApiPhase::Listening).await;

    exit.send(()).unwrap();
    let failed = wait_for_lifecycle(&mut lifecycle, |s| s.phase == RestApiPhase::Failed).await;
    assert_eq!(failed.generation, initial.generation);
    assert_eq!(failed.diagnostic, Some(RestApiDiagnostic::ListenerExited));
    assert_recoverable_failure(&failed, "exited unexpectedly");
    let recovered = wait_for_lifecycle(&mut lifecycle, |s| {
        s.phase == RestApiPhase::Listening && s.generation > failed.generation
    })
    .await;
    assert_eq!(recovered.generation, failed.generation + 1);
    let recovery = tokio::time::timeout(std::time::Duration::from_secs(5), recovery_events.recv())
        .await
        .expect("recovery signal deadline elapsed")
        .expect("recovery signal owner stopped");
    assert_eq!(recovery.status.generation, recovered.generation);
    assert_eq!(recovery.transition_id, 1);
    let response = reqwest::get(format!("http://{address}/api/health"))
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    control.shutdown_and_wait().await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn shutdown_error_fails_its_restart_completion_then_recovers() {
    let _socket_owner = REAL_SOCKET_TEST.lock().await;
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let control = Arc::new(RestApiControl::new());
    control.force_shutdown_error();
    let mut lifecycle = control.subscribe_lifecycle();
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    wait_for_lifecycle(&mut lifecycle, |s| s.phase == RestApiPhase::Listening).await;

    let restart_control = control.clone();
    let restart = tokio::spawn(async move { restart_control.restart_and_wait().await.unwrap() });
    let failed = wait_for_lifecycle(&mut lifecycle, |s| s.phase == RestApiPhase::Failed).await;
    let completed = restart.await.unwrap();
    assert_eq!(completed, failed);
    assert_recoverable_failure(&completed, "shutdown failed");
    let recovered = wait_for_lifecycle(&mut lifecycle, |s| {
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
    let _socket_owner = REAL_SOCKET_TEST.lock().await;
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let control = Arc::new(RestApiControl::new());
    let mut status = control.subscribe_status();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    control.set_restart_barrier(barrier.clone());
    let task = tokio::spawn(start(control.clone(), address.to_string(), health_router()));
    wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;

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
    entered: std::sync::Barrier,
    release: std::sync::Barrier,
}

fn send_blocking_request(address: SocketAddr, path: &'static str) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
    })
}

fn assert_health_is_unanswered(address: SocketAddr) {
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    write!(
        stream,
        "GET /api/health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut byte = [0_u8; 1];
    let error = stream
        .read(&mut byte)
        .expect_err("blocked workers must not produce a health response");
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ));
}

#[tokio::test]
async fn diagnostics_distinguish_listener_exit_from_worker_stall() {
    let _socket_owner = REAL_SOCKET_TEST.lock().await;
    let exit_address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let exit_control = Arc::new(RestApiControl::new());
    let exit = exit_control.install_server_exit();
    let mut exit_events = exit_control.subscribe_lifecycle();
    let exit_task = tokio::spawn(start(
        exit_control.clone(),
        exit_address.to_string(),
        health_router(),
    ));
    wait_for_lifecycle(&mut exit_events, |s| s.phase == RestApiPhase::Listening).await;
    exit.send(()).unwrap();
    let listener_failure =
        wait_for_lifecycle(&mut exit_events, |s| s.phase == RestApiPhase::Failed).await;
    assert_eq!(
        listener_failure.diagnostic,
        Some(RestApiDiagnostic::ListenerExited)
    );
    exit_control.shutdown_and_wait().await.unwrap();
    exit_task.await.unwrap();

    let stall_address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let stall_control = Arc::new(RestApiControl::new());
    let mut stall_events = stall_control.subscribe_lifecycle();
    let gates = Arc::new(WorkerGates {
        entered: std::sync::Barrier::new(5),
        release: std::sync::Barrier::new(5),
    });
    let stall_gates = gates.clone();
    let router = health_router().route(
        "/stall",
        axum::routing::get(move || {
            let gates = stall_gates.clone();
            async move {
                gates.entered.wait();
                gates.release.wait();
                "released"
            }
        }),
    );
    let server_control = stall_control.clone();
    let server = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(start(server_control, stall_address.to_string(), router));
    });
    let listening =
        wait_for_lifecycle(&mut stall_events, |s| s.phase == RestApiPhase::Listening).await;
    let clients = (0..4)
        .map(|_| send_blocking_request(stall_address, "/stall"))
        .collect::<Vec<_>>();
    let entered = gates.clone();
    tokio::task::spawn_blocking(move || entered.entered.wait())
        .await
        .unwrap();
    tokio::task::spawn_blocking(move || assert_health_is_unanswered(stall_address))
        .await
        .unwrap();

    assert_eq!(
        stall_control.record_unanswered_probe(),
        RestApiDiagnostic::WorkersBlocked
    );
    let worker_failure =
        wait_for_lifecycle(&mut stall_events, |s| s.phase == RestApiPhase::Failed).await;
    assert_eq!(worker_failure.generation, listening.generation);
    assert_eq!(
        worker_failure.diagnostic,
        Some(RestApiDiagnostic::WorkersBlocked)
    );
    assert_ne!(worker_failure.diagnostic, listener_failure.diagnostic);
    assert_ne!(stall_control.status().phase, RestApiPhase::Listening);

    let release = gates.clone();
    tokio::task::spawn_blocking(move || release.release.wait())
        .await
        .unwrap();
    for client in clients {
        client.join().unwrap();
    }
    let response = reqwest::get(format!("http://{stall_address}/api/health"))
        .await
        .unwrap();
    assert!(response.status().is_success());
    stall_control.record_successful_probe();
    assert_eq!(stall_control.status().phase, RestApiPhase::Listening);
    assert_eq!(stall_control.status().generation, listening.generation);

    stall_control.shutdown_and_wait().await.unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn repeated_same_generation_outages_emit_exactly_one_resync_each() {
    let control = RestApiControl::new();
    let mut recovery_events = control.take_recovery_events().unwrap();
    let generation = control.begin_generation();
    control.listener_bound(generation);
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
