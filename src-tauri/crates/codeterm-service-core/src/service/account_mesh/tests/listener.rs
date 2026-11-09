use super::*;
use crate::startup::rest_api::{self, RestApiControl, RestApiPhase};
use std::net::{Ipv4Addr, SocketAddr};

/// Stand-in tailnet address: 127.0.0.1 on its own port, since macOS lo0 carries no 127.0.0.2.
fn tailnet_stand_in() -> SocketAddr {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("reserve a loopback port")
}

struct LoopbackAlias(watch::Receiver<()>, SocketAddr);

#[async_trait]
impl tailnet::AddressChanges for LoopbackAlias {
    async fn selected(&self, enabled: bool, port: u16) -> Vec<SocketAddr> {
        if enabled && port != 0 {
            vec![self.1]
        } else {
            Vec::new()
        }
    }

    async fn changed(&mut self) -> Result<(), ()> {
        self.0.changed().await.map_err(|_| ())
    }
}

/// Detection that can fail transiently and never emits a native address notification.
struct FlakyDetection(Arc<std::sync::atomic::AtomicBool>, SocketAddr);

#[async_trait]
impl tailnet::AddressChanges for FlakyDetection {
    async fn selected(&self, enabled: bool, port: u16) -> Vec<SocketAddr> {
        if enabled && port != 0 && self.0.load(std::sync::atomic::Ordering::SeqCst) {
            vec![self.1]
        } else {
            Vec::new()
        }
    }

    async fn changed(&mut self) -> Result<(), ()> {
        std::future::pending().await
    }
}

fn connected_transport() -> codeterm_mesh::network_transport::TransportStatus {
    codeterm_mesh::network_transport::TransportStatus {
        display_name: "Test network".into(),
        state: codeterm_mesh::network_transport::TransportState::Connected,
        identity: None,
        network_name: None,
        user: None,
        addresses: vec![Ipv4Addr::new(100, 64, 1, 2).into()],
        peer_listener: true,
        encrypted: true,
        setup_actions: Vec::new(),
    }
}

#[tokio::test]
async fn tailnet_listener_recovers_when_another_observer_records_the_transport() {
    tokio::time::timeout(Duration::from_secs(30), transport_recovery())
        .await
        .expect("tailnet transport recovery deadline");
}

async fn transport_recovery() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = Arc::new(fixture.service.clone());
    let mesh = service.ext::<crate::MeshStateHandle>().clone();
    {
        let mut config = service.ext::<crate::config::ConfigState>().lock();
        config.mesh.enabled = true;
        config.api.enabled = true;
    }
    let router = axum::Router::new().route("/api/health", axum::routing::get(|| async { "ok" }));
    let control = Arc::new(RestApiControl::new());
    service
        .set_ext(control.clone())
        .unwrap_or_else(|_| panic!("install listener controller"));
    service
        .set_ext(crate::ports::rest::LocalApiRouter(router.clone()))
        .unwrap_or_else(|_| panic!("install production router"));
    let (published, mut ready) = watch::channel(Vec::new());
    control.listeners.configure(
        None,
        Arc::new(move |addresses| {
            published.send_replace(addresses);
        }),
    );
    let mut status = control.subscribe_status();
    let server = rest_api::spawn(control.clone(), "127.0.0.1:0".into(), router);
    while status.borrow_and_update().phase != RestApiPhase::Listening {
        status.changed().await.unwrap();
    }
    let remote = tailnet_stand_in();
    let detected = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let transport = mesh.lock().transport_watch();
    let task = tokio::spawn(tailnet::run_with_network(
        service.clone(),
        FlakyDetection(detected.clone(), remote),
        transport,
    ));
    wait_for_ready(&mut ready, remote, true).await;
    detected.store(false, std::sync::atomic::Ordering::SeqCst);
    service
        .events
        .publish_generic("config-changed", serde_json::Value::Null);
    wait_for_ready(&mut ready, remote, false).await;
    detected.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(mesh.lock().set_network_transport(connected_transport()));
    wait_for_ready(&mut ready, remote, true).await;
    task.abort();
    let _ = task.await;
    control.shutdown_and_wait().await.unwrap();
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
}

async fn wait_for_ready(
    changes: &mut watch::Receiver<Vec<SocketAddr>>,
    address: SocketAddr,
    ready: bool,
) {
    loop {
        if changes.borrow_and_update().contains(&address) == ready {
            return;
        }
        changes.changed().await.unwrap();
    }
}

async fn wait_for_connectivity(
    events: &mut tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>,
) {
    loop {
        if matches!(events.recv().await.unwrap().event, codeterm_events::SystemEvent::Generic { event_type, .. } if event_type == reconcile::CONNECTIVITY_CHANGED)
        {
            return;
        }
    }
}

#[tokio::test]
async fn tailnet_production_lifecycle_binds_signed_out_and_preserves_socket_on_sign_out() {
    tokio::time::timeout(Duration::from_secs(30), lifecycle())
        .await
        .expect("production listener lifecycle deadline");
}

async fn lifecycle() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = Arc::new(fixture.service.clone());
    let mesh = service.ext::<crate::MeshStateHandle>().clone();
    let account = listener_account_mesh(
        &service,
        Arc::new(FileAccountMeshStore::new(
            fixture.temp.path().join("account-mesh"),
        )),
    );
    account.load().await;
    account.sign_out(async {}).await.1.unwrap();
    let identity = codeterm_account::Identity {
        uid: "listener-owner".into(),
        email: None,
        display_name: None,
        provider: "test".into(),
        photo_url: None,
    };
    {
        let mut config = service.ext::<crate::config::ConfigState>().lock();
        config.mesh.enabled = true;
        config.api.enabled = true;
    }
    let router = axum::Router::new().route(
        "/api/health",
        axum::routing::get(|| async { "production listener" }),
    );
    let control = Arc::new(RestApiControl::new());
    service
        .set_ext(control.clone())
        .unwrap_or_else(|_| panic!("install listener controller"));
    service
        .set_ext(crate::ports::rest::LocalApiRouter(router.clone()))
        .unwrap_or_else(|_| panic!("install production router"));
    let (published, mut ready) = watch::channel(Vec::new());
    let observed_mesh = mesh.clone();
    control.listeners.configure(
        None,
        Arc::new(move |addresses| {
            observed_mesh.lock().set_ready_listeners(addresses.clone());
            published.send_replace(addresses);
        }),
    );
    let mut status = control.subscribe_status();
    let server = rest_api::spawn(control.clone(), "127.0.0.1:0".into(), router);
    loop {
        if status.borrow_and_update().phase == RestApiPhase::Listening {
            break;
        }
        status.changed().await.unwrap();
    }
    let remote = tailnet_stand_in();
    let (network_change, notifications) = watch::channel(());
    let mut network_events = service.events.subscribe();
    let task = tokio::spawn(tailnet::run_with_network(
        service.clone(),
        LoopbackAlias(notifications, remote),
        mesh.lock().transport_watch(),
    ));
    wait_for_ready(&mut ready, remote, true).await;
    wait_for_connectivity(&mut network_events).await;
    while network_events.try_recv().is_ok() {}
    network_change.send_replace(());
    wait_for_connectivity(&mut network_events).await;
    #[cfg(windows)]
    let child = tokio::task::spawn_blocking(
        codeterm_util::tcp_listener::test_support::InheritingChild::start,
    )
    .await
    .unwrap();
    account.identity_changed(Some(&identity)).await;
    account.sign_out(async {}).await.1.unwrap();
    assert!(
        codeterm_util::tcp_listener::bind(remote).is_err(),
        "sign-out preserves the web listener"
    );
    #[cfg(windows)]
    assert!(child.is_alive());
    account.identity_changed(Some(&identity)).await;
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{remote}/api/health"))
        .header(reqwest::header::CONNECTION, "close")
        .send()
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "production listener");
    account.sign_out(async {}).await.1.unwrap();
    while network_events.try_recv().is_ok() {}
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .mesh
        .enabled = false;
    service
        .events
        .publish_generic("config-changed", serde_json::Value::Null);
    wait_for_ready(&mut ready, remote, false).await;
    wait_for_connectivity(&mut network_events).await;
    let rebound =
        codeterm_util::tcp_listener::bind(remote).expect("Mesh disable releases the listener");
    drop(rebound);
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .mesh
        .enabled = true;
    service
        .events
        .publish_generic("config-changed", serde_json::Value::Null);
    wait_for_ready(&mut ready, remote, true).await;
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .api
        .enabled = false;
    service
        .events
        .publish_generic("config-changed", serde_json::Value::Null);
    wait_for_ready(&mut ready, remote, false).await;
    drop(codeterm_util::tcp_listener::bind(remote).expect("API disable releases the listener"));
    task.abort();
    let _ = task.await;
    control.shutdown_and_wait().await.unwrap();
    tokio::task::spawn_blocking(move || server.join().unwrap())
        .await
        .unwrap();
    #[cfg(windows)]
    tokio::task::spawn_blocking(move || drop(child))
        .await
        .unwrap();
}
