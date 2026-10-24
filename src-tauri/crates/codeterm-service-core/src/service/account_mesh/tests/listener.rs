use super::*;
use crate::startup::rest_api::{self, RestApiControl, RestApiPhase};
use std::net::{Ipv4Addr, SocketAddr};

struct LoopbackAlias;

#[async_trait]
impl tailnet::AddressChanges for LoopbackAlias {
    async fn selected(&self, signed_in: bool, enabled: bool, port: u16) -> Vec<SocketAddr> {
        if signed_in && enabled && port != 0 {
            vec![(Ipv4Addr::new(127, 0, 0, 2), port).into()]
        } else {
            Vec::new()
        }
    }

    async fn changed(&mut self) -> Result<(), ()> {
        std::future::pending().await
    }
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

#[tokio::test]
async fn tailnet_production_lifecycle_sign_out_releases_socket_while_child_lives() {
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
    let identity = codeterm_account::Identity {
        uid: "listener-owner".into(),
        email: None,
        display_name: None,
        provider: "test".into(),
        photo_url: None,
    };
    account.identity_changed(Some(&identity)).await;
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
    let remote = SocketAddr::new(
        Ipv4Addr::new(127, 0, 0, 2).into(),
        control.bound_addr().unwrap().port(),
    );
    let task = tokio::spawn(tailnet::run_with_network(
        service.clone(),
        mesh,
        LoopbackAlias,
    ));
    wait_for_ready(&mut ready, remote, true).await;
    #[cfg(windows)]
    let child = tokio::task::spawn_blocking(
        codeterm_util::tcp_listener::test_support::InheritingChild::start,
    )
    .await
    .unwrap();
    account.sign_out(async {}).await.1.unwrap();
    wait_for_ready(&mut ready, remote, false).await;
    let rebound = codeterm_util::tcp_listener::bind(remote)
        .expect("sign-out must close the real production listener even while children live");
    #[cfg(windows)]
    assert!(child.is_alive());
    drop(rebound);
    account.identity_changed(Some(&identity)).await;
    wait_for_ready(&mut ready, remote, true).await;
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
    wait_for_ready(&mut ready, remote, false).await;
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
